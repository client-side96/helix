use std::time::Duration;

use helix_core::{syntax::config::LanguageServerFeature, text_annotations::InlineAnnotation};
use helix_event::{cancelable_future, register_hook, send_blocking, TaskController, TaskHandle};
use helix_lsp::{
    lsp::InlineCompletionResponse,
    util::{lsp_pos_to_pos, pos_to_lsp_pos},
};
use helix_view::{
    document::Mode,
    handlers::{inline_completion::InlineCompletionEvent, Handlers},
    DocumentId, Editor, ViewId,
};
use tokio::time::Instant;

use crate::{
    compositor::Compositor,
    events::{OnModeSwitch, PostInsertChar},
    job::{dispatch, dispatch_blocking},
};

#[derive(Debug, Clone, Copy)]
pub(super) struct Trigger {
    pub(super) view: ViewId,
    pub(super) doc: DocumentId,
}
pub struct InlineCompletionHandler {
    trigger: Option<Trigger>,
    in_flight: Option<Trigger>,
    task_controller: TaskController,
}

impl InlineCompletionHandler {
    pub fn new() -> InlineCompletionHandler {
        Self {
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
                cursor: _cursor,
                doc,
                view,
            } => {
                if self
                    .trigger
                    .or(self.in_flight)
                    .is_none_or(|trigger| trigger.doc != doc || trigger.view != view)
                {
                    self.trigger = Some(Trigger { view, doc });
                }

                self.trigger
                    .map(|_trigger| Instant::now() + Duration::from_millis(200))
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
    let doc_id = doc.id();
    let view_id = view.id;
    let text = doc.text();
    let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
    let language_servers: Vec<_> = doc
        .language_servers_with_feature(LanguageServerFeature::InlineCompletion)
        .collect();

    for ls in language_servers.iter() {
        let offset_encoding = ls.offset_encoding();
        let pos = pos_to_lsp_pos(text, cursor, offset_encoding);
        if let Some(response) = ls.inline_completion(doc.identifier(), pos, None) {
            let request = async move {
                match response.await {
                    Ok(res) => {
                        dispatch(move |editor, _compositor| {
                            let items = match res {
                                Some(InlineCompletionResponse::Array(items)) => items,
                                Some(InlineCompletionResponse::List(list)) => list.items,
                                None => vec![],
                            };

                            if let Some(item) = items.into_iter().next() {
                                let doc = doc_mut!(editor, &doc_id);
                                let text = doc.text();

                                let ghost_text = if let Some(ref range) = item.range {
                                    if let Some(range_start) =
                                        lsp_pos_to_pos(text, range.start, offset_encoding)
                                    {
                                        let already_typed = cursor.saturating_sub(range_start);
                                        item.insert_text
                                            [already_typed.min(item.insert_text.len())..]
                                            .to_string()
                                    } else {
                                        item.insert_text.clone()
                                    }
                                } else {
                                    item.insert_text.clone()
                                };

                                let annotations = vec![InlineAnnotation::new(cursor, ghost_text)];
                                doc.set_inline_completion(
                                    view_id,
                                    helix_view::document::InlineCompletionAnnotation {
                                        item,
                                        annotations,
                                    },
                                );
                            }
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

pub(super) fn register_hooks(_handlers: &Handlers) {
    register_hook!(move |event: &mut PostInsertChar<'_, '_>| {
        let (view, doc) = current_ref!(event.cx.editor);
        let view_id = view.id;
        let doc_id = doc.id();
        let text = doc.text();
        let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
        let doc = doc_mut!(event.cx.editor, &doc_id);
        doc.clear_inline_completion(view_id);

        send_blocking(
            &event.cx.editor.handlers.inline_completions,
            InlineCompletionEvent::AutoTrigger {
                cursor,
                doc: doc.id(),
                view: view.id,
            },
        );
        Ok(())
    });

    register_hook!(move |event: &mut OnModeSwitch<'_, '_>| {
        let (view, doc) = current_ref!(event.cx.editor);
        let view_id = view.id;
        let doc_id = doc.id();
        let text = doc.text();
        let cursor = doc.selection(view.id).primary().cursor(text.slice(..));
        let doc = doc_mut!(event.cx.editor, &doc_id);

        if event.old_mode == Mode::Insert {
            doc.clear_inline_completion(view_id);
            send_blocking(
                &event.cx.editor.handlers.inline_completions,
                InlineCompletionEvent::Cancel,
            );
        }

        if event.new_mode == Mode::Insert {
            doc.clear_inline_completion(view_id);

            send_blocking(
                &event.cx.editor.handlers.inline_completions,
                InlineCompletionEvent::AutoTrigger {
                    cursor,
                    doc: doc.id(),
                    view: view.id,
                },
            );
        }
        Ok(())
    });
}
