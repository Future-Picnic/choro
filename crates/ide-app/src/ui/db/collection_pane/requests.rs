//! Collection page requests. The page and filter on screen are committed
//! only by a successful, current request; a failed page change, reload or
//! filter leaves the previous documents, selection, page and query in place
//! and is kept so Retry repeats exactly what failed. Paging and reloads use
//! the committed filter, never an unsubmitted draft in the input.

use std::time::Duration;

use gpui::App;

use super::*;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DocRequest {
    pub page: u64,
    pub filter: String,
}

impl CollectionPane {
    /// Reloads the page on screen with its committed filter.
    pub(super) fn load(&mut self, cx: &mut Context<Self>) {
        self.load_page(self.page, cx);
    }

    pub(super) fn load_page(&mut self, page: u64, cx: &mut Context<Self>) {
        let request = self.page_request(page);
        self.load_request(request, cx);
    }

    /// Submits the filter typed in the input, from the first page.
    pub(super) fn submit_filter(&mut self, cx: &mut Context<Self>) {
        let request = self.draft_request(cx);
        self.load_request(request, cx);
    }

    pub(super) fn draft_request(&self, cx: &App) -> DocRequest {
        DocRequest {
            page: 0,
            filter: self.filter_input.read(cx).value().trim().to_string(),
        }
    }

    /// Repeats the request that failed, or reloads the page on screen.
    pub(super) fn retry_load(&mut self, cx: &mut Context<Self>) {
        let request = self
            .pending_request
            .clone()
            .unwrap_or_else(|| self.page_request(self.page));
        self.load_request(request, cx);
    }

    pub(super) fn page_request(&self, page: u64) -> DocRequest {
        DocRequest {
            page,
            filter: self.filter.clone(),
        }
    }

    fn load_request(&mut self, request: DocRequest, cx: &mut Context<Self>) {
        let seq = self.begin_request(request.clone());
        let handle = self.handle.clone();
        let db = self.db.clone();
        let collection = self.collection.clone();
        let skip = request.page * PAGE_SIZE;
        let filter = request.filter.clone();
        cx.spawn(async move |this, cx| {
            let (result, elapsed) = cx
                .background_executor()
                .spawn(async move {
                    let started = std::time::Instant::now();
                    let result =
                        handle.find_docs(&db, &collection, &filter, skip, PAGE_SIZE as i64);
                    (result, started.elapsed())
                })
                .await;
            this.update(cx, |this, cx| {
                if this.finish_request(seq, request, result, elapsed) {
                    cx.notify();
                }
            })
            .ok();
        })
        .detach();
        cx.notify();
    }

    /// Marks a request in flight and returns its sequence number. Results on
    /// screen stay until it succeeds.
    pub(super) fn begin_request(&mut self, request: DocRequest) -> u64 {
        self.pending_request = Some(request);
        self.loading = true;
        self.error = None;
        self.load_seq += 1;
        self.load_seq
    }

    /// Applies a finished request if it is still the latest. Returns whether
    /// anything changed.
    pub(super) fn finish_request(
        &mut self,
        seq: u64,
        request: DocRequest,
        result: anyhow::Result<ide_core::DocPage>,
        elapsed: Duration,
    ) -> bool {
        if self.load_seq != seq {
            return false;
        }
        self.loading = false;
        match result {
            Ok(page) => {
                self.page = request.page;
                self.filter = request.filter;
                self.pending_request = None;
                self.fetch_ms = Some(elapsed.as_millis());
                self.selected_doc = (!page.docs.is_empty()).then_some(0);
                self.expanded_tree_nodes.clear();
                self.docs = page.docs.into_iter().map(doc_card).collect();
                self.total = page.total;
            }
            Err(error) => self.error = Some(format!("{error:#}").into()),
        }
        true
    }

    /// What a failed request was trying to do, so the retained results are
    /// never mistaken for its answer.
    pub(super) fn failed_request_label(&self) -> Option<SharedString> {
        let error = self.error.as_ref()?;
        let request = self.pending_request.as_ref();
        let attempt = match request {
            Some(request) if request.filter != self.filter => "The filter was not applied",
            Some(request) if request.page != self.page => {
                return Some(
                    format!("Page {} could not be loaded. {error}", request.page + 1).into(),
                );
            }
            _ => "Reload failed",
        };
        Some(format!("{attempt}. {error}").into())
    }
}

fn doc_card(entry: ide_core::DocEntry) -> DocCard {
    let (display, clipped) = clip_for_display(&entry.json);
    let tree = serde_json::from_str::<Value>(&entry.json).ok();
    DocCard {
        display: display.into(),
        clipped,
        full: entry.json.into(),
        tree,
        id: entry.id,
    }
}
