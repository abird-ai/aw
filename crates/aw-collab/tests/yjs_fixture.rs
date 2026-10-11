use aw_collab::DraftDocument;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use serde::Deserialize;

#[derive(Deserialize)]
struct Fixture {
    text: String,
    yjs_13_6_33_update_v1_b64: String,
    yrs_0_28_0_small_client_update_v1_b64: String,
}

#[test]
fn yjs_and_yrs_v1_fixtures_materialize_identically() {
    let fixture: Fixture = serde_json::from_str(include_str!(
        "../../../tests/fixtures/shared_draft_yjs_v1.json"
    ))
    .unwrap();
    for encoded in [
        &fixture.yjs_13_6_33_update_v1_b64,
        &fixture.yrs_0_28_0_small_client_update_v1_b64,
    ] {
        let bytes = BASE64.decode(encoded).unwrap();
        let draft = DraftDocument::from_update_v1(&bytes).unwrap();
        assert_eq!(draft.text(), fixture.text);
    }
}
