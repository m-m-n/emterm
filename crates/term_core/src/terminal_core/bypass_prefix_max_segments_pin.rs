use super::replay_plan::BYPASS_PREFIX_MAX_SEGMENTS;

/// The two segments a fold-succeeded (`h > 0`) MIDDLE can never itself
/// claim from the wire cap: the mandatory HEAD (`candidate_h > 0`) and
/// the mandatory SUFFIX (`k < segments.len()`). See the module doc
/// above.
const MANDATORY_HEAD_AND_SUFFIX_SEGMENTS: usize = 2;

#[test]
fn bypass_prefix_max_segments_matches_the_fold_succeeded_ceiling() {
    let expected = mux_ipc::protocol::MAX_SEGMENTS - MANDATORY_HEAD_AND_SUFFIX_SEGMENTS;
    assert_eq!(
        BYPASS_PREFIX_MAX_SEGMENTS,
        expected,
        "BYPASS_PREFIX_MAX_SEGMENTS ({BYPASS_PREFIX_MAX_SEGMENTS}) must equal \
         mux_ipc::protocol::MAX_SEGMENTS ({}) minus \
         MANDATORY_HEAD_AND_SUFFIX_SEGMENTS ({MANDATORY_HEAD_AND_SUFFIX_SEGMENTS}) \
         = {expected} — the largest MIDDLE a fold-succeeded (h > 0) split can ever \
         contain for a legal daemon snapshot. If this fired because \
         BYPASS_PREFIX_MAX_SEGMENTS fell below {expected}: that is the round-7/ \
         round-8 regression (gate left behind while the daemon's segment cap moved \
         up) unless paired with a documented re-derivation here. If it fired because \
         BYPASS_PREFIX_MAX_SEGMENTS rose above {expected}: a daemon snapshot can \
         never legally produce a fold-succeeded MIDDLE this large, so the gate would \
         admit an unreachable (or non-daemon) shape without the cost evidence that \
         would justify it. Either way, this is NOT the constant to lower for a \
         deliberate h == 0 cost-policy decision — see \
         BYPASS_PREFIX_MAX_SEGMENTS_UNFOLDED_HEAD, which carries no such pin.",
        mux_ipc::protocol::MAX_SEGMENTS
    );
}
