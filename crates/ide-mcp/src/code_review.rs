//! The review server exposes exactly four tools, and never workspace reads.
use super::*;
use anyhow::ensure;
use ide_core::code_review::*;
#[cfg(test)]
#[path = "code_review_tests.rs"]
mod tests;

pub(super) fn allowed(name: &str) -> bool {
    matches!(
        name,
        "review_context" | "review_read" | "review_search" | "review_report"
    )
}
pub(super) fn binding(ctx: &ServerContext) -> Result<ReviewRun> {
    let run_id = ctx.review_run.context("This server is not a reviewer")?;
    let run = ctx.store()?.load_review_run(run_id)?;
    authorize_review(
        &run,
        ctx.project_id()?.0,
        ctx.agent_id()?,
        run_id,
        review_now(),
    )?;
    Ok(run)
}
/// A replay of the exact finalization acknowledges the existing outcome. It
/// cannot reopen reads, accept late findings, or finalize a host timeout.
pub(super) fn finalization_replay(ctx: &ServerContext, params: &Value) -> Option<Value> {
    if params["name"] != "review_report"
        || params["arguments"] != json!({"report":{"action":"finalize"}})
    {
        return None;
    }
    let id = ctx.review_run?;
    let run = ctx.store().ok()?.load_review_run(id).ok()?;
    if run.version != REVIEW_VERSION
        || run.reviewer_id != ctx.agent_id?
        || run.project_id != ctx.project_id?
        || !run.finalized_by_reviewer
        || !matches!(
            run.state,
            ReviewRunState::Complete | ReviewRunState::Partial
        )
    {
        return None;
    }
    Some(
        json!({"content":[text_content(json!({"accepted":true,"duplicate":true,"state":run.state}).to_string())],"isError":false}),
    )
}
pub(super) fn tools() -> Vec<Box<dyn Tool>> {
    [
        "review_context",
        "review_read",
        "review_search",
        "review_report",
    ]
    .into_iter()
    .map(|name| Box::new(ReviewTool(name)) as Box<dyn Tool>)
    .collect()
}
struct ReviewTool(&'static str);
impl Tool for ReviewTool {
    fn name(&self) -> &'static str {
        self.0
    }
    fn title(&self) -> &'static str {
        self.0
    }
    fn description(&self) -> &'static str {
        match self.0 {
            "review_context" => "Start with a compact overview. Full decisions, check evidence and file metadata are available in paginated sections; none are discarded.",
            "review_read" => "Prefer kind=batch with batch ID and zero-based page to consume related diff pages together. Individual diff pages and numbered snapshot source remain available. No live workspace access.",
            "review_search" => "Bounded literal search within frozen snapshot source. Optional repository-relative prefix narrows scope.",
            _ => "Report stage, completed file, candidate, checked finding, discard or finalization. File completion requires all assigned diff pages. Only validated checked findings become visible.",
        }
    }
    fn input_schema(&self) -> Value {
        match self.0 {
            "review_context" => {
                json!({"type":"object","additionalProperties":false,"properties":{"section":{"enum":["overview","decisions","checks","files","omissions"]},"page":{"type":"integer","minimum":0}}})
            }
            "review_read" => {
                json!({"type":"object","additionalProperties":false,"properties":{"kind":{"enum":["batch","diff","source"]},"batch":{"type":"integer","minimum":0},"file_id":{"type":"string"},"page":{"type":"integer","minimum":0},"path":{"type":"string"},"side":{"enum":["Before","After","Source"]},"start":{"type":"integer","minimum":1},"end":{"type":"integer","minimum":1}},"required":["kind"]})
            }
            "review_search" => {
                json!({"type":"object","additionalProperties":false,"properties":{"query":{"type":"string","minLength":1,"maxLength":256},"prefix":{"type":"string"}},"required":["query"]})
            }
            _ => {
                json!({"type":"object","additionalProperties":false,"properties":{"report":{"type":"object","description":"action: stage (stage Understanding/Reviewing/CheckingFindings, optional batch); files_complete (file_ids: one to five IDs, all assigned pages consumed); file_complete (file_id); candidate/checked (finding: id,severity Critical/High/Medium/Low,location {file_id,path,side Before/After,start,end},title,trigger,consequence,suggested_fix,evidence [{path,file_id,side,content_hash,start,end,excerpt}],challenge); discard (finding_id,reason); finalize."}},"required":["report"]})
            }
        }
    }
    fn call(&self, ctx: &ServerContext, args: &Value) -> Result<Vec<Value>> {
        ensure!(
            serde_json::to_vec(args)?.len() <= 1024 * 1024,
            "Review tool request exceeds 1 MiB"
        );
        let bound = binding(ctx)?;
        let store = ctx.store()?;
        let input = store.load_review_input(&bound)?;
        let storage = store.review_storage(bound.id);
        let (_,result) = store.transact_review(bound.id, None, |run| {
            // Recheck under the writer reservation: Cancel may have raced with
            // the first binding check. Source is not exposed after cancellation.
            authorize_review(run, ctx.project_id()?.0, ctx.agent_id()?, bound.id, review_now())?;
            validate_input(run, &input)?;
            match self.0 {
                "review_context" => {
                    reject_unknown_arguments(args, &["section","page"])?;
                    let section = args.get("section").map(|v| v.as_str().context("section must be a string")).transpose()?.unwrap_or("overview");
                    let page = args.get("page").map(|v| v.as_u64().context("page must be nonnegative")).transpose()?.unwrap_or(0);
                    ensure!(page <= usize::MAX as u64, "Invalid context page");
                    review_context(run, &input, section, page as usize)
                },
                "review_read" => {
                    reject_unknown_arguments(args, &["kind","batch","file_id","page","path","side","start","end"])?;
                    match args["kind"].as_str() {
                        Some("batch") => {
                            let batch = args["batch"].as_u64().context("batch is required")?;
                            let page = args["page"].as_u64().context("page is required")?;
                            ensure!(batch <= usize::MAX as u64 && page <= usize::MAX as u64, "Invalid batch page");
                            review_batch_page(run, &input, &storage, batch as usize, page as usize)
                        },
                        Some("diff") => {
                            let file_id = args["file_id"].as_str().context("file_id is required")?;
                            let page = args["page"].as_u64().context("page is required")?;
                            ensure!(page <= usize::MAX as u64, "Invalid page");
                            let text = review_diff_page(run, &input, &storage, file_id, page as usize)?;
                            Ok(json!({"file_id":file_id,"page":page,"total_pages":input.diffs[file_id].len(),"content_hash":input.diffs[file_id][page as usize].content_hash,"text":text}))
                        },
                        Some("source") => {
                            let path = Path::new(args["path"].as_str().context("path is required")?);
                            let side: ReviewSide = serde_json::from_value(args.get("side").cloned().unwrap_or(json!("Source")))?;
                            let start = args["start"].as_u64().context("start is required")?;
                            let end = args["end"].as_u64().context("end is required")?;
                            ensure!(start <= u32::MAX as u64 && end <= u32::MAX as u64, "Invalid line range");
                            review_source(run, &input, &storage, path, args["file_id"].as_str(), side, start as u32, end as u32)
                        },
                        _ => anyhow::bail!("kind must be batch, diff or source"),
                    }
                },
                "review_search" => {
                    reject_unknown_arguments(args, &["query","prefix"])?;
                    review_search(run, &input, &storage, args["query"].as_str().context("query is required")?, args["prefix"].as_str().map(Path::new))
                },
                _ => {
                    reject_unknown_arguments(args, &["report"])?;
                    let report: ReviewReport = serde_json::from_value(args["report"].clone())?;
                    // Freshness is revalidated by the host before results are
                    // presented/fixed. The reviewer cannot mark them current.
                    apply_review_report(run, &input, &storage, report, review_now())?;
                    Ok(json!({"accepted":true,"state":run.state,"stage":run.stage,"completed_files":run.completed_files(),"total_files":run.total_files(),"checked_findings":run.findings.len(),"pending_candidate_ids":run.candidates.iter().map(|f| &f.id).collect::<Vec<_>>()}))
                },
            }
        })?;
        Ok(vec![text_content(serde_json::to_string(&result)?)])
    }
}
