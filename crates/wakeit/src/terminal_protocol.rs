//! Ephemeral terminal transport. Keystrokes never enter JetStream or execution events.
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

pub const LEASE_SECONDS: u64 = 30;
pub const FRAME_BYTES: usize = 32 * 1024;
pub const BUFFER_BYTES: usize = 8 * 1024 * 1024;

pub fn control_subject(runner: Uuid) -> String {
    format!("_INBOX.r_{}.terminal", runner.simple())
}
pub fn output_subject(runner: Uuid, terminal: Uuid) -> String {
    format!(
        "{}.terminal.{}",
        crate::protocol::reply_prefix(runner),
        terminal.simple()
    )
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Control {
    pub version: u32,
    pub terminal_id: Uuid,
    pub workspace_id: Uuid,
    pub runner_id: Uuid,
    pub runtime_id: Uuid,
    pub generation: i64,
    pub expires_at: i64,
    pub action: String,
    #[serde(default)]
    pub data: Value,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Frame {
    pub version: u32,
    pub workspace_id: Uuid,
    pub terminal_id: Uuid,
    pub runtime_id: Uuid,
    pub generation: i64,
    pub seq: u64,
    pub kind: String,
    pub data: Value,
}
pub fn dimensions(data: &Value) -> anyhow::Result<(u16, u16)> {
    let cols = data["cols"].as_u64().unwrap_or(80);
    let rows = data["rows"].as_u64().unwrap_or(24);
    anyhow::ensure!(
        (2..=500).contains(&cols) && (2..=200).contains(&rows),
        "invalid terminal size"
    );
    Ok((cols as u16, rows as u16))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn dimensions_are_bounded_before_allocating_a_screen() {
        assert_eq!(dimensions(&json!({})).unwrap(), (80, 24));
        assert_eq!(
            dimensions(&json!({"cols":500,"rows":200})).unwrap(),
            (500, 200)
        );
        for value in [
            json!({"cols":0}),
            json!({"rows":201}),
            json!({"cols":65536}),
        ] {
            assert!(dimensions(&value).is_err());
        }
    }
    #[test]
    fn terminal_subjects_stay_outside_reliable_streams_and_inside_device_permissions() {
        let runner = Uuid::new_v4();
        let terminal = Uuid::new_v4();
        assert!(control_subject(runner).starts_with(&format!("_INBOX.r_{}.", runner.simple())));
        let output = output_subject(runner, terminal);
        assert!(output.starts_with(&(crate::protocol::reply_prefix(runner) + ".")));
        for subject in [
            crate::protocol::event_subject(runner),
            crate::protocol::work_subject(runner),
            crate::protocol::control_subject(runner),
        ] {
            assert_ne!(output, subject);
            assert_ne!(control_subject(runner), subject);
        }
    }
    #[test]
    fn snapshot_restores_unicode_cursor_and_native_input_modes() {
        let mut parser = vt100::Parser::new(24.try_into().unwrap(), 80.try_into().unwrap(), 20);
        parser.process("中文\r\nline\x1b[?2004h\x1b[?1h\x1b[3;4H".as_bytes());
        let snapshot = parser.screen().state_formatted();
        let mut recovered = vt100::Parser::new(24.try_into().unwrap(), 80.try_into().unwrap(), 20);
        recovered.process(snapshot.as_bytes());
        assert_eq!(recovered.screen().contents(), parser.screen().contents());
        assert_eq!(
            recovered.screen().cursor_position(),
            parser.screen().cursor_position()
        );
        assert_eq!(
            recovered.screen().input_mode_formatted(),
            parser.screen().input_mode_formatted()
        );
    }
}
