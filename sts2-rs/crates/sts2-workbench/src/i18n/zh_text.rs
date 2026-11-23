//! Chinese for the engine's own words (see `crate::i18n`). Keys are the
//! English text passed to `tr` / `trf` / `tr_dyn`.
//!
//! This table covers the advisors (card / decision advisor, choice ranker,
//! regret review, turn recap), SimLab, the seed checker, the state importer
//! and the user-facing API errors.

pub(super) const TABLE: &[(&str, &str)] = &[
    // ── advisor option / action labels ────────────────────────────────
    ("slot {slot}", "栏位 {slot}"),
    ("end turn", "结束回合"),
    ("drink {name}", "饮用{name}"),
    ("drink potion slot {slot}", "饮用栏位 {slot} 的药水"),
    ("card {n}", "卡牌 {n}"),
    ("action {n}", "行动 {n}"),
    ("skip", "跳过"),
    ("option {n}", "选项 {n}"),
    ("buy card {n}", "购买卡牌 {n}"),
    ("buy relic {n}", "购买遗物 {n}"),
    ("buy potion {n}", "购买药水 {n}"),
    ("leave", "离开"),
    ("relic {n}", "遗物 {n}"),
    ("drink potion {n}", "饮用药水 {n}"),
    ("discard potion {n}", "丢弃药水 {n}"),
    ("take reward {n}", "领取奖励 {n}"),
    ("skip reward {n}", "跳过奖励 {n}"),
    ("proceed", "继续"),
    // ── advisor errors / notes ────────────────────────────────────────
    (
        "the choice ranker only runs on an in-combat card-select choice with at least two legal options",
        "选牌排序只在战斗中至少有两个合法选项的选牌时可用",
    ),
    (
        "no native combat state (the choice ranker requires the Rust engine backend)",
        "没有原生战斗状态（选牌排序需要 Rust 引擎后端）",
    ),
    ("no native combat state", "没有原生战斗状态"),
    ("no live run", "当前没有进行中的一局"),
    ("no MCTS scores available", "没有可用的 MCTS 评分"),
    (
        "card advisor only runs on a plain card-reward decision",
        "卡牌顾问只在普通的卡牌奖励时可用",
    ),
    (
        "the decision advisor only runs on a shop, event, or card-select decision with at least two options",
        "决策顾问只在至少有两个选项的商店、事件或选牌时可用",
    ),
    // ── the turn recap's reason line ──────────────────────────────────
    ("wins the fight", "赢下战斗"),
    ("enemy {n}", "敌人 {n}"),
    ("kills {name}", "击杀{name}"),
    ("wakes {name}", "唤醒{name}"),
    ("your line {what}", "你的路线{what}"),
    ("the AI's line {what}", "AI 的路线{what}"),
    ("{a} and {b}", "{a}并{b}"),
    ("{a}; {b}", "{a}；{b}"),
    ("{text} on turn {turn}", "第{turn}回合：{text}"),
    // ── regret review ─────────────────────────────────────────────────
    (
        "Turn {turn}: {played} matched the MCTS line (P(win) {pct}%)",
        "第{turn}回合：{played}与 MCTS 路线一致（胜率 {pct}%）",
    ),
    (
        "Turn {turn}: {played} instead of {best} dropped P(win) {from}%→{to}% (−{drop} pts), ≈{hp} HP",
        "第{turn}回合：打出{played}而不是{best}，胜率 {from}%→{to}%（−{drop} 点），≈{hp} 生命",
    ),
    (
        "Turn {turn}: {played} instead of {best} dropped P(win) {from}%→{to}% (−{drop} pts)",
        "第{turn}回合：打出{played}而不是{best}，胜率 {from}%→{to}%（−{drop} 点）",
    ),
    // ── intents (the turn context) ────────────────────────────────────
    ("attack {damage}×{hits}", "攻击 {damage}×{hits}"),
    ("attack {damage}", "攻击 {damage}"),
    // ── the MCTS pick rule's reason (`PickRule::reason`, via tr_dyn) ──
    (
        "the most damage in a fight the search sees as lost",
        "在搜索判定必败的战斗中造成最多伤害",
    ),
    ("a proven win", "已证明必胜"),
    ("the highest value", "价值最高"),
    ("the most searched of the equal moves", "同等走法中搜索次数最多"),
    ("block before ending the turn into damage", "受到伤害前先格挡再结束回合"),
    ("play the free card before ending the turn", "结束回合前先打出免费的牌"),
    ("get rid of the curse or status", "处理掉诅咒或状态牌"),
    ("playing it first is never worse", "先打出它绝不会更差"),
    ("keep the potion for a fight that needs it", "把药水留给需要它的战斗"),
    ("spend the leftover energy before ending the turn", "结束回合前用掉剩余能量"),
    ("play the upgraded copy", "打出已升级的那张"),
    ("the search's choice is not legal here", "搜索的选择在此处不合法"),
    // ── SimLab ────────────────────────────────────────────────────────
    ("the engine rejected this position: {error}", "引擎拒绝了这个局面：{error}"),
    (
        "{where}: {id} loses {what} (not carried by the position).",
        "{where}：{id} 失去{what}（局面不保存这些）。",
    ),
    (
        "not an engine combat wire (no player / deck block)",
        "不是引擎战斗数据（没有 player / deck 块）",
    ),
    ("victory", "胜利"),
    ("defeat", "失败"),
    (
        "that fight is already over ({phase}); nothing to play from",
        "这场战斗已经结束（{phase}）；无从继续",
    ),
    (
        "a card is mid-resolution (play pile is not empty); copy the state at a decision point",
        "有一张牌正在结算（出牌区不为空）；请在决策点复制状态",
    ),
    (
        "Draw-pile pinned top pins are not carried; the order is.",
        "抽牌堆的置顶固定不会保存；顺序会保存。",
    ),
    (
        "Draw-pile pinned bottom pins are not carried; the order is.",
        "抽牌堆的置底固定不会保存；顺序会保存。",
    ),
    ("The deferred draw-order flag is not carried.", "延迟抽牌顺序标记不会保存。"),
    (
        "{kind}: the wire telegraphs a move without naming it; the intent will be re-rolled unless you pick one.",
        "{kind}：数据显示了意图却没有给出招式名；除非你选一个，否则意图会重新随机。",
    ),
    ("{kind}: {key} is not carried by the position.", "{kind}：局面不保存 {key}。"),
    ("HP ({hp}) is above max HP ({max_hp})", "生命（{hp}）高于最大生命（{max_hp}）"),
    ("the position has no cards in any pile", "这个局面的所有牌堆都没有牌"),
    ("the position has no enemies", "这个局面没有敌人"),
    ("pick an encounter or name at least one monster", "请选择一场遭遇，或至少指定一个怪物"),
    ("nothing pasted", "没有粘贴任何内容"),
    (
        "That is a live-game fight paste the engine could not rebuild. {why}",
        "这是一段实时游戏的战斗数据，引擎无法重建。{why}",
    ),
    (
        "That state has no fight in it. Paste an engine combat state (or start a New fight) to load it here.",
        "这个状态里没有战斗。请粘贴引擎战斗状态（或开始一场新战斗）以在此载入。",
    ),
    (" → {target}", " → {target}"),
    ("Play {name}{target}", "打出{name}{target}"),
    ("End turn", "结束回合"),
    ("potion {n}", "药水 {n}"),
    ("Drink {name}{target}", "饮用{name}{target}"),
    ("Channel {orb}{target}", "生成{orb}{target}"),
    ("Evoke ×{n}", "激发 ×{n}"),
    ("Choose #{n}", "选择 #{n}"),
    (
        "opens a choice; cannot sweep from mid-resolution",
        "会打开一个选择；无法从结算中途开始扫描",
    ),
    // Encounter tiers (the catalog's optgroups).
    ("Weak", "较弱"),
    ("Normal", "普通"),
    ("Elite", "精英"),
    ("Boss", "首领"),
    ("Unknown", "未知"),
    (
        "invalid deck name {name} (must be non-empty and contain no '/')",
        "无效的牌组名 {name}（不能为空，也不能包含“/”）",
    ),
    ("deck {name} is empty", "牌组 {name} 是空的"),
    ("deck {name} has {n} cards (max {max})", "牌组 {name} 有 {n} 张牌（最多 {max} 张）"),
    ("deck {name}: unknown card id {card}", "牌组 {name}：未知的卡牌 id {card}"),
    // ── seed checker route notes ──────────────────────────────────────
    ("got {name}", "获得{name}"),
    ("took {name}", "拿了{name}"),
    ("skipped the card", "跳过了卡牌"),
    ("skipped the relic", "跳过了遗物"),
    ("opened the chest", "打开了宝箱"),
    ("boss relic: {name}", "首领遗物：{name}"),
    ("bought {name}", "买了{name}"),
    ("removed {name}", "移除了{name}"),
    ("upgraded {name}", "升级了{name}"),
    ("transformed {name}", "变化了{name}"),
    ("rested", "休息了"),
    ("smithed {name}", "锻造了{name}"),
    // ── state import: what reproduces exactly, warnings, errors ───────
    ("seed", "种子"),
    ("character", "角色"),
    ("ascension", "进阶"),
    ("deck", "牌组"),
    ("relics", "遗物"),
    ("potions", "药水"),
    ("gold", "金币"),
    ("hp", "生命"),
    ("map + position", "地图 + 位置"),
    (
        "combat board (hand, piles, powers, intents)",
        "战斗盘面（手牌、牌堆、能力、意图）",
    ),
    (
        "The fight could not be rebuilt ({error}). Copy at a decision point rather than mid-animation.",
        "无法重建这场战斗（{error}）。请在决策点复制，而不是在动画进行中。",
    ),
    (
        "{n} deck card(s) this engine does not know were left out: {list}",
        "已略去 {n} 张本引擎不认识的牌组卡牌：{list}",
    ),
    (
        "{n} relic(s) this engine does not model were left out: {list}",
        "已略去 {n} 个本引擎未实现的遗物：{list}",
    ),
    (
        "{n} potion(s) this engine does not model were left out: {list}",
        "已略去 {n} 瓶本引擎未实现的药水：{list}",
    ),
    ("Nothing to import — the paste is empty.", "没有可导入的内容——粘贴内容为空。"),
    (
        "That does not look like JSON. Paste one wire message, one log line, or a whole JSONL log.",
        "这看起来不是 JSON。请粘贴一条数据消息、一行日志或整份 JSONL 日志。",
    ),
    (
        "This message carries no run_state, so there is no run to rebuild. Copy a state while a run is in progress.",
        "这条消息没有 run_state，因此没有可重建的一局。请在一局进行中复制状态。",
    ),
    (
        "This message names no character, so it was copied outside a run.",
        "这条消息没有指明角色，所以它是在一局之外复制的。",
    ),
    (
        "Unknown character {character} — this engine has no port.",
        "未知角色 {character}——本引擎没有移植它。",
    ),
    (
        "This message carries no run seed, so the map and every roll would belong to a different run.",
        "这条消息没有本局种子，地图和每次随机都会属于另一局。",
    ),
    (
        "No RNG cursors in this paste — the map and deck are exact, but the next card reward, shop and event will be this engine's rolls, not the live run's.",
        "粘贴内容中没有随机数游标——地图和牌组是精确的，但接下来的卡牌奖励、商店和事件将由本引擎随机生成，而不是实时游戏的结果。",
    ),
    (
        "No map graph in this paste — the run will start at the act entrance instead of where you were standing.",
        "粘贴内容中没有地图——这一局会从本幕入口开始，而不是你所在的位置。",
    ),
    (
        "That is an engine wire dict, not a live-game state. It has no run attached (no seed, deck or map), so there is no run to rebuild — copy the state from the mod instead.",
        "这是引擎数据字典，不是实时游戏状态。它不包含一局（没有种子、牌组或地图），因此没有可重建的一局——请改为从模组复制状态。",
    ),
    (
        "That is a headless combat_play decision, which carries no run (no seed, deck or map). Copy the state from the mod instead.",
        "这是无界面的 combat_play 决策，不包含一局（没有种子、牌组或地图）。请改为从模组复制状态。",
    ),
    (
        "No game state in that paste. Expected a SpireMentor wire message (advice_request / combat_action / a screen), an advisor log line, or a whole advisor log.",
        "粘贴内容中没有游戏状态。应为 SpireMentor 数据消息（advice_request / combat_action / 某个界面）、一行顾问日志或整份顾问日志。",
    ),
    // ── API errors the page shows ─────────────────────────────────────
    (
        "the run-level AI (run policy, advisors, autoplay) is not available in Blitz mode",
        "闪电战模式下不提供本局层面的 AI（路线策略、顾问、自动游玩）",
    ),
    ("in Blitz mode the AI answers in a fight only", "闪电战模式下 AI 只在战斗中提供建议"),
    ("no run loaded", "未载入任何一局"),
    (
        "the seed checker only plays {version} for now, not the Fool",
        "种子检查目前只支持 {version}，不支持愚者",
    ),
    (
        "the seed checker only plays {version} for now, not {other}",
        "种子检查目前只支持 {version}，不支持 {other}",
    ),
    ("ascension must be 0-10, not {value}", "进阶必须在 0-10 之间，而不是 {value}"),
    ("hint only available during combat", "提示只在战斗中可用"),
    ("search tree only available during combat", "搜索树只在战斗中可用"),
    (
        "choice ranker only available at an in-combat card-select",
        "选牌排序只在战斗中选牌时可用",
    ),
    ("no recommendation here", "此处没有推荐"),
    ("no gradable turn just ended", "刚结束的回合无法评分"),
    ("no prior run-level choice to grade", "没有可评分的上一个本局选择"),
    ("card advisor only available at a card reward", "卡牌顾问只在卡牌奖励时可用"),
    (
        "decision advisor only available at a shop, event, or choice",
        "决策顾问只在商店、事件或选择时可用",
    ),
    ("not at a card reward", "当前不是卡牌奖励"),
    ("not at an advisable decision", "当前不是可分析的决策"),
    ("nothing to check", "没有可检查的内容"),
    ("bench needs a live combat", "基准测试需要进行中的战斗"),
    (
        "could not start the {backend} engine: {error}",
        "无法启动 {backend} 引擎：{error}",
    ),
    (
        "this paste carries no fight — it was copied on the {surface} screen. Import it as a run instead.",
        "这段粘贴内容中没有战斗——它是在 {surface} 界面复制的。请改为作为一局导入。",
    ),
    ("could not rebuild this state: {error}", "无法重建这个状态：{error}"),
    ("could not rebuild this run: {error}", "无法重建这一局：{error}"),
    ("save name must not be blank", "存档名不能为空"),
    ("unknown save", "未知存档"),
    ("cannot restore this save: {error}", "无法恢复这个存档：{error}"),
    ("cannot restore this session: {error}", "无法恢复这个会话：{error}"),
    // ── PPO / Instinct ────────────────────────────────────────────────
    (
        "no PPO net loaded for {character}: {reason}",
        "未为 {character} 载入 PPO 网络：{reason}",
    ),
    ("no PPO net loaded for {character}", "未为 {character} 载入 PPO 网络"),
    ("Remove {name}{plus} (deck #{n})", "移除{name}{plus}（牌组 #{n}）"),
    (
        "Instinct plays {characters} only (not {name})",
        "直觉只支持 {characters}（不支持 {name}）",
    ),
    ("no Instinct net loaded: {reason}", "未载入直觉网络：{reason}"),
    (
        "no Instinct net loaded ({file} not fetched yet)",
        "未载入直觉网络（尚未获取 {file}）",
    ),
];
