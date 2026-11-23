//! Stamp `SRC_HASH` (see `sts2_srchash::emit_build_stamp`).

fn main() {
    sts2_srchash::emit_build_stamp("sts2-advisor", "STS2_ADVISOR_SRC_HASH");
}
