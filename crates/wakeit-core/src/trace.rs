//! Compatibility exports for the shared plugin trace contract.
pub use wakeit_plugin_protocol::trace::{
    Lines, LogRecord, RECORD_LIMIT, RecordData, RecordTooLarge, RecordingIssue, StreamRecord,
    TraceKind, TraceNode, TraceState, recording_issue,
};
