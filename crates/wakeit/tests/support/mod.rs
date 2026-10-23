use serde_json::Value;
use wakeit::trace::{RecordData, StreamRecord};

// Assert the complete finish contract; empty or duplicate reports must fail.
pub fn assert_single_report(records: &[StreamRecord], expected: Value) {
    assert_eq!(
        records.len(),
        1,
        "expected exactly one final report: {records:?}"
    );
    let RecordData::Result(actual) = &records[0].record else {
        panic!("expected a final report, got {:?}", records[0]);
    };
    assert_eq!(actual, &expected);
}
