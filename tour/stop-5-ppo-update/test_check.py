import math
import sys
from pathlib import Path

import pytest

sys.path.insert(0, str(Path(__file__).resolve().parent))
import check  # noqa: E402


def stats_row(it, phase, decisions, floor, kl, clipfrac, ent_n, t):
    return {
        "it": it,
        "decisions": decisions,
        "floor_mean": floor,
        "dec_per_s": 100.0,
        "time": t,
        "update": {
            "phase": phase,
            "probe_kl": kl,
            "clipfrac": clipfrac,
            "ent_n": ent_n,
            "ev_combat": 0.1,
        },
    }


ROWS = [
    check.parse_row(stats_row(1, "V-warm", 1000, 1.8, 0.0, None, None, 110.0)),
    check.parse_row(stats_row(2, "U0", 2000, 2.0, 0.004, 0.0, 0.99, 120.0)),
    check.parse_row(stats_row(3, "U0", 3000, 2.5, 0.010, 0.02, 0.95, 130.0)),
    check.parse_row(stats_row(4, "U0", 4000, 3.0, 0.008, 0.01, 0.93, 140.0)),
    check.parse_row(stats_row(5, "U0", 5000, 3.2, 0.006, 0.04, 0.91, 150.0)),
]


def test_missing_statistics_become_nan_not_errors():
    r = ROWS[0]
    assert r.phase == "V-warm" and math.isnan(r.clipfrac) and math.isnan(r.entropy)


def test_summary_skips_value_only_iterations_and_uses_the_learners_windows():
    verdict = {"first_mean": 1.9, "last_mean": 3.1, "complete": True}
    s = check.summarize(ROWS, verdict, t_start=100.0)
    assert (s.floor_start, s.floor_end, s.complete) == (1.9, 3.1, True)
    assert s.kl_median == pytest.approx(0.007)  # median of the four U0 rows
    assert s.clipfrac_median == pytest.approx(0.015)
    assert s.entropy_change == pytest.approx((0.95 + 0.93 + 0.91) / 3 - 0.99)
    assert s.dps == pytest.approx(5000 / 50.0)  # start-up counts against throughput


def test_an_arm_that_stopped_early_still_summarises():
    s = check.summarize(ROWS[:1], {}, t_start=100.0)
    assert math.isnan(s.kl_median) and math.isnan(s.floor_start) and not s.complete


def test_table_has_one_line_per_iteration():
    assert len(check.table(ROWS).splitlines()) == 1 + len(ROWS)


@pytest.mark.parametrize("arm", list(check.ARMS))
@pytest.mark.parametrize("size", [check.DEFAULT, check.QUICK])
def test_each_arm_is_a_valid_learner_command(arm, size, tmp_path):
    """The learner's own parser and `Cfg` accept the command line, and the arm's
    changes reach the config the learner would train with."""
    from ppo.e2e import learner as L
    from ppo.e2e import schedules as S

    argv = check.learner_argv(arm, size, tmp_path / arm, seed=0)
    assert argv[1:4] == ["-m", "ppo.e2e.learner", "train"]
    cfg = L.cfg_from_args(L.build_parser().parse_args(argv[3:]))
    a = check.ARMS[arm]
    assert cfg.mode == "learning_check" and cfg.init == "" and cfg.teacher_pair == ""
    assert cfg.clip == a["clip"] and cfg.epochs == a["epochs"]
    sched = cfg.schedule()
    assert sched.lr_trunk == pytest.approx(S.DEFAULT.lr_trunk * a["lr_scale"])
    assert S.anneal(1_800_000_000.0, sched) == 1.0  # no anneal on any day of the tour
    assert cfg.net == check.SMALL_NET and cfg.max_wall_s == size.wall_s


def test_unknown_arms_are_refused_before_any_training(monkeypatch, tmp_path):
    monkeypatch.setenv("TOUR_DEV", "1")
    monkeypatch.setattr(sys, "argv", ["check.py", "--arms", "clipped,nope", "--keep", str(tmp_path / "k")])
    monkeypatch.setattr(check, "run_arms", lambda *a, **k: pytest.fail("trained"))
    with pytest.raises(SystemExit, match="unknown arms"):
        check.main()
    assert not (tmp_path / "k").exists()
