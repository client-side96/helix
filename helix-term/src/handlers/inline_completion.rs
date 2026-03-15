use std::{sync::Arc, time::Duration};

use arc_swap::ArcSwap;
use helix_core::syntax::config::LanguageServerFeature;
use helix_event::{cancelable_future, register_hook, send_blocking, TaskController, TaskHandle};
use helix_lsp::util::pos_to_lsp_pos;
use helix_view::{
    handlers::{inline_completion::InlineCompletionEvent, Handlers},
    DocumentId, Editor, ViewId,
};
use tokio::time::Instant;

use crate::{
    compositor::Compositor,
    config::Config,
    events::PostInsertChar,
    job::{dispatch, dispatch_blocking},
};

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub(super) enum TriggerKind {
    Auto,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct Trigger {
    pub(super) pos: usize,
    pub(super) view: ViewId,
    pub(super) doc: DocumentId,
    pub(super) kind: TriggerKind,
}
pub struct InlineCompletionHandler {
    trigger: Option<Trigger>,
    in_flight: Option<Trigger>,
    task_controller: TaskController,
    config: Arc<ArcSwap<Config>>,
}

impl InlineCompletionHandler {
    pub fn new(config: Arc<ArcSwap<Config>>) -> InlineCompletionHandler {
        Self {
            config,
            task_controller: TaskController::new(),
            trigger: None,
            in_flight: None,
        }
    }
}

impl helix_event::AsyncHook for InlineCompletionHandler {
    type Event = InlineCompletionEvent;

    fn handle_event(
        &mut self,
        event: Self::Event,
        _old_timeout: Option<Instant>,
    ) -> Option<Instant> {
        if self.in_flight.is_some() && !self.task_controller.is_running() {
            self.in_flight = None;
        }
        match event {
            InlineCompletionEvent::AutoTrigger {
                cursor: trigger_pos,
                doc,
                view,
            } => {
                if self
                    .trigger
                    .or(self.in_flight)
                    .is_none_or(|trigger| trigger.doc != doc || trigger.view != view)
                {
                    self.trigger = Some(Trigger {
                        pos: trigger_pos,
                        view,
                        doc,
                        kind: TriggerKind::Auto,
                    });
                }

                self.trigger
                    .map(|_trigger| Instant::now() + Duration::from_millis(500))
            }
            InlineCompletionEvent::Cancel => {
                self.trigger = None;
                self.task_controller.cancel();
                None
            }
        }
    }

    fn finish_debounce(&mut self) {
        let trigger = self.trigger.take().expect("debounce always has a trigger");
        self.in_flight = Some(trigger);
        let handle = self.task_controller.restart();
        dispatch_blocking(move |editor, compositor| {
            request_inline_completions(trigger, handle, editor, compositor);
        });
    }
}

fn request_inline_completions(
    _trigger: Trigger,
    handle: TaskHandle,
    editor: &mut Editor,
    _compositor: &mut Compositor,
) {
    let (view, doc) = current_ref!(editor);
    let text = doc.text();
    let language_servers: Vec<_> = doc
        .language_servers_with_feature(LanguageServerFeature::InlineCompletion)
        .collect();
    for ls in language_servers.iter() {
        let offset_encoding = ls.offset_encoding();
        let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
        let pos = pos_to_lsp_pos(text, cursor, offset_encoding);
        if let Some(response) = ls.inline_completion(doc.identifier(), pos, None) {
            let request = async move {
                match response.await {
                    Ok(res) => {
                        dispatch(move |_editor, _compositor| {
                            log::info!("Got inline completion: {:?}", res);
                        })
                        .await;
                    }
                    Err(err) => log::error!("Inline completion failed: {}", err),
                }
            };
            tokio::spawn(cancelable_future(request, handle.clone()));
        }
    }
}

pub(super) fn register_hooks(handlers: &Handlers) {
    let tx = handlers.inline_completions.clone();

    register_hook!(move |event: &mut PostInsertChar<'_, '_>| {
        let (view, doc) = current_ref!(event.cx.editor);
        let text = doc.text();
        let cursor = doc.selection(view.id).primary().cursor(text.slice(..));

        send_blocking(
            &tx,
            InlineCompletionEvent::AutoTrigger {
                cursor,
                doc: doc.id(),
                view: view.id,
            },
        );
        Ok(())
    });
}
