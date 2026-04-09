use crate::{DocumentId, ViewId};

pub enum InlineCompletionEvent {
    AutoTrigger {
        cursor: usize,
        doc: DocumentId,
        view: ViewId,
    },
    Cancel,
}
