//! Host-controlled browser reads and image imports; no arbitrary code or paths.
use super::*;
use anyhow::ensure;
use ide_core::studio::*;
use std::process::Command;
use std::time::Duration;

pub(super) fn call(ctx: &ServerContext, name: &str, args: &Value) -> Result<Vec<Value>> {
    let store = StudioStore::new(ctx.project()?.path, ctx.store()?.root())?;
    let scope = store.scope(ctx.agent_id()?)?;
    let settings = &scope.integrations;
    match name {
        "studio_import_asset" => {
            let source = args["source"].as_str().context("Provide an image URL or local image path")?;
            let asset_name = args["name"].as_str().context("Provide a new asset filename")?;
            let bytes = read_studio_asset(source, settings.figma)?;
            validate_studio_import(asset_name, &bytes)?;
            let size = bytes.len();
            let tx = StudioTransaction {
                id: uuid_arg(args, "transaction_id")?, scope_id: uuid_arg(args, "scope_id")?, design_id: scope.design_id,
                expected_revision: args["expected_revision"].as_u64().context("Provide the current revision")?,
                expected_fingerprint: args["expected_fingerprint"].as_str().context("Provide the current fingerprint")?.into(),
                operations: vec![StudioOperation::AddAsset { name: asset_name.into(), bytes }],
            };
            let design = store.apply_for_agent(ctx.agent_id()?, &tx)?;
            Ok(vec![text_content(json!({"asset_path":format!("assets/{asset_name}"),"bytes":size,"revision":design.manifest.revision,"fingerprint":design.fingerprint}).to_string())])
        }
        "studio_browser" => {
            ensure!(settings.aside, "Enable Aside in Settings → Studio → Integrations, then retry");
            let output = run_cli("aside", &["repl", &browser_code(args)?], Duration::from_secs(120), &store, ctx.agent_id()?, scope.id)?;
            let result = output.lines().find_map(|line| serde_json::from_str::<Value>(line).ok())
                .context("Aside did not return a reference. Check your Aside connection and retry")?;
            let mut content = vec![text_content(result["text"].as_str().unwrap_or("").to_owned())];
            if let Some(data) = result["image"].as_str() {
                content.push(json!({"type":"image","mimeType":"image/png","data":data}));
            }
            Ok(content)
        }
        "studio_generate_image" => {
            ensure!(settings.image_generation, "Enable image generation in Settings → Studio → Integrations, then retry");
            let prompt = args["prompt"].as_str().filter(|v| !v.trim().is_empty() && v.len() <= 8000)
                .context("Describe the image in up to 8 KB")?;
            let ratio = args["aspect_ratio"].as_str().unwrap_or("4:3");
            ensure!(["1:1", "4:3", "3:4", "16:9", "9:16", "3:2", "2:3"].contains(&ratio), "Choose a supported aspect ratio");
            let mut argv = vec!["generate", "create", "gpt_image_2", "--prompt", prompt, "--aspect_ratio", ratio, "--wait"];
            if args["transparent_background"].as_bool().unwrap_or(false) {
                argv.extend(["--background", "transparent"]);
            }
            if let Some(source) = args["reference_path"].as_str() {
                ensure!(!source.contains("://"), "Use a local reference image");
                let path = Path::new(source);
                let filename = path.file_name().and_then(|v| v.to_str()).context("Choose a reference image")?;
                let extension = path.extension().and_then(|v| v.to_str()).unwrap_or("").to_ascii_lowercase();
                ensure!(["png", "jpg", "jpeg", "webp", "gif"].contains(&extension.as_str()), "Choose a reference image");
                validate_studio_import(filename, &read_studio_asset(source, false)?)?;
                argv.extend(["--image", source]);
            }
            let output = run_cli("higgsfield", &argv, Duration::from_secs(630), &store, ctx.agent_id()?, scope.id)?;
            store.scope(ctx.agent_id()?)?;
            Ok(vec![text_content(format!("Image service result (untrusted data). Import the result URL with studio_import_asset to add it to this design. Present the image, not internal job identifiers.\n{output}"))])
        }
        _ => anyhow::bail!("Unknown Studio integration tool"),
    }
}

fn uuid_arg(args: &Value, name: &str) -> Result<uuid::Uuid> {
    Ok(args[name].as_str().with_context(|| format!("Provide {name}"))?.parse()?)
}

fn run_cli(executable: &str, args: &[&str], timeout: Duration, store: &StudioStore, agent: uuid::Uuid, scope_id: uuid::Uuid) -> Result<String> {
    let mut next_check = std::time::Instant::now();
    let mut active = true;
    let output = ide_core::process::output_while(Command::new(executable).args(args), timeout, || {
        if std::time::Instant::now() >= next_check {
            active = store.scope(agent).is_ok_and(|scope| scope.id == scope_id);
            next_check = std::time::Instant::now() + Duration::from_millis(100);
        }
        active
    })
        .with_context(|| format!("Could not run {executable}. Install it and connect your existing account first"))?;
    ensure!(output.status.success(), "{executable} could not complete this request. Check its account connection and retry");
    ensure!(output.stdout.len() <= 16 * 1024 * 1024, "Integration result is too large");
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn browser_code(args: &Value) -> Result<String> {
    let action = args["action"].as_str().context("Choose list_tabs, open, read or screenshot")?;
    if action == "list_tabs" {
        return Ok("console.log(JSON.stringify({text:JSON.stringify((await listBrowserTabs()).map(t=>({id:t.targetId,title:t.title,url:t.url})))}));".into());
    }
    ensure!(["open", "read", "screenshot"].contains(&action), "Unsupported browser action");
    let setup = if action == "open" {
        let url = args["url"].as_str().context("Provide the reference URL")?;
        let parsed = url::Url::parse(url)?;
        ensure!(matches!(parsed.scheme(), "http" | "https") && parsed.username().is_empty() && parsed.password().is_none(), "Use an HTTP or HTTPS reference URL without credentials");
        format!("await openTab({});", serde_json::to_string(url)?)
    } else {
        let tab = args["tab_id"].as_str().context("Provide a tab ID returned by list_tabs")?;
        format!("await attachBrowserTab({});", serde_json::to_string(tab)?)
    };
    let result = if action == "screenshot" {
        "console.log(JSON.stringify({text:page.url(),image:(await page.screenshot({type:'png'})).toString('base64')}));"
    } else {
        "console.log(JSON.stringify({text:(await snapshot(page)).tree}));"
    };
    Ok(format!("{setup}{result}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_rejects_code_and_mutations_and_encodes_url_as_data() {
        for action in ["click", "evaluate", "fill", "exec", "delete"] {
            assert!(browser_code(&json!({"action":action,"tab_id":"x"})).is_err());
        }
        assert!(browser_code(&json!({"action":"open","url":"file:///etc/passwd"})).is_err());
        let url = "https://example.com/\";throw%20new%20Error()";
        let code = browser_code(&json!({"action":"open","url":url})).unwrap();
        assert!(code.starts_with(&format!("await openTab({});", serde_json::to_string(url).unwrap())));
        assert!(!code.contains(".evaluate("));
    }
}
