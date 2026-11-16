use super::*;

#[test]
fn a_patch_id_is_a_whole_sha_1() {
    assert_eq!(
        PatchId::parse(&"AB".repeat(20)).map(|id| id.as_str().to_owned()),
        Some("ab".repeat(20))
    );
    assert_eq!(PatchId::parse("abc1234"), None);
    assert_eq!(PatchId::parse(&"g".repeat(40)), None);
}
