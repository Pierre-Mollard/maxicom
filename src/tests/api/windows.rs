//! The named pipe's permissions (MAX-74 step 2)
use super::*;

/// MAX-74 (step 2): this user's SID is read from the process token, and the pipe's descriptor
/// allows it alone, nothing inherited; Windows accepts the descriptor.
/// How: the SID's shape, the SDDL text, the descriptor built.
#[test]
fn owner_only_allows_this_user() {
    let sid = user_sid().unwrap();
    assert!(sid.starts_with("S-1-5-"), "{sid}");
    assert_eq!(owner_only_sddl(&sid), format!("D:P(A;;GA;;;{sid})"));
    owner_only().unwrap();
}
