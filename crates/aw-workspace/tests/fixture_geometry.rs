//! The expected frames are shared with a separate Python structural fixture.
//! Neither fixture measures an actual terminal; Ratatui/PTY integration is separate.
use aw_workspace::{Rect, ScopeId, Workspace};
#[test]
fn shared_layout_geometry_fixture() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/geometry.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let workspace: Workspace = serde_json::from_value(case["workspace"].clone()).unwrap();
        let workspace = workspace.migrate(&ScopeId::launch()).unwrap();
        let rect: Rect = serde_json::from_value(case["area"].clone()).unwrap();
        let actual = workspace
            .geometry(rect, case["maximize"].as_bool().unwrap())
            .unwrap();
        assert_eq!(
            actual.hidden as u64,
            case["hidden"].as_u64().unwrap(),
            "{}",
            case["name"]
        );
        let got = actual
            .visible
            .iter()
            .map(|p| serde_json::json!({"pane":p.pane,"area":p.area}))
            .collect::<Vec<_>>();
        assert_eq!(serde_json::json!(got), case["visible"], "{}", case["name"]);
    }
}
