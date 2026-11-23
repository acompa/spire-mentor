// The combat board and its chrome (Simplified Chinese): Board, Combatant,
// CardFrame, the HUD, trays, the history log's sentences and turn ledger,
// the intent breakdown. Keys are the English source text.
const zh: Record<string, string> = {
  // Board
  "draw pile ({n})": "抽牌堆（{n}）",
  "discard pile ({n})": "弃牌堆（{n}）",
  "exhaust pile ({n})": "消耗牌堆（{n}）",
  "energy {n}/{max}": "能量 {n}/{max}",
  "draw": "抽牌",
  "discard": "弃牌",
  "exhaust": "消耗",
  "End Turn (E)": "结束回合（E）",
  "End Turn (replay)": "结束回合（回放）",
  "End Turn {n}": "结束回合 {n}",
  "step {n}": "第{n}步",
  "play best (G)": "打出最佳（G）",
  "autoplay (A)": "自动出牌（A）",
  "play the MCTS-recommended move every step until the fight ends (win or lose)":
    "每一步都执行 MCTS 推荐的操作，直到战斗结束（无论胜负）",
  "play to end (F)": "打到结束（F）",
  "inspect the MCTS search tree": "查看 MCTS 搜索树",
  "search (S)": "搜索（S）",
  "thinking…": "思考中…",
  "select a target…": "选择目标…",
  "auto-played {name}": "自动打出 {name}",
  "auto-played": "自动打出",
  "Draw pile": "抽牌堆",
  "order hidden — sorted, not draw order": "顺序隐藏——已排序，并非抽牌顺序",
  "Discard pile": "弃牌堆",
  "Exhaust pile": "消耗牌堆",

  // Combatant
  "the AI's target": "AI 的目标",
  "Intent: {name}": "意图：{name}",
  "Stolen from your deck: {card} — kill {enemy} before it escapes to get it back":
    "从你的牌组中偷走：{card}——在{enemy}逃走前击杀它即可夺回",
  "{enemy} stole {card}": "{enemy}偷走了{card}",
  "More powers": "更多能力",

  // CardFrame
  "star": "辉星",
  "energy": "能量",
  "no art": "无卡图",
  "cannot be played": "无法打出",
  "enchanted: {name}": "附魔：{name}",
  "costs {n} stars": "消耗 {n} 辉星",
  "{n} copies": "{n} 张",
  "played": "已打出",

  // PotionTray / Powers / RelicTray / Orbs
  "empty potion slot": "空药水栏",
  "use": "使用",
  "discard {name}": "丢弃{name}",
  "{n} more powers": "另有 {n} 个能力",
  "{n} more relics": "另有 {n} 个遗物",
  "fewer relics": "收起遗物",
  "empty orb slot": "空充能球栏位",
  " — passive {passive}, evoke {evoke}": " — 被动 {passive}，激发 {evoke}",

  // HealthBar
  "block {n}": "格挡 {n}",
  "Block {n}": "格挡 {n}",
  "Block": "格挡",
  "Absorbs damage before it reaches HP.": "在伤害扣除生命之前将其吸收。",

  // ZoomCard
  "Play {name}": "打出{name}",
  "{name}, zoomed": "{name}（放大）",

  // GameHud
  "Ascension {n}": "进阶 {n}",
  "Dread mode. {dominion} acts on this act's fights, up to its boss.":
    "恐惧模式。{dominion}作用于本幕的战斗，直到首领战为止。",
  "Dread mode. No dominion is in force here.": "恐惧模式。此处没有生效的支配。",
  "Blitz mode. Each act is a fixed line of eight rooms; rooms pay no gold. ":
    "极速模式。每一幕是固定的一条八个房间的路线；房间不给金币。",
  "hp {hp}/{max}": "生命 {hp}/{max}",
  "HP {hp}/{max}": "生命 {hp}/{max}",
  "gold {n}": "金币 {n}",
  "Gold {n}": "金币 {n}",
  "map": "地图",
  "show the act map": "显示本幕地图",
  "deck {n}": "牌组 {n}",
  "view deck ({n} cards)": "查看牌组（{n} 张牌）",

  // logLine: the history log's sentences (names and amounts slot in)
  "{actor} dealt {amount} damage to {target} using {source}.": "{actor}使用{source}对{target}造成了{amount}点伤害。",
  "{actor} dealt {amount} damage to {target}.": "{actor}对{target}造成了{amount}点伤害。",
  "{target} took {amount} damage.": "{target}受到了{amount}点伤害。",
  "{actor} gained {amount} {noun}.": "{actor}获得了{amount}点{noun}。",
  "{actor} applied {amount} {noun} to {target}.": "{actor}给予{target}{amount}层{noun}。",
  "{target} gained {amount} {noun}.": "{target}获得了{amount}层{noun}。",
  "{actor} used {noun}.": "{actor}使用了{noun}。",
  "{actor} drank {noun}.": "{actor}喝下了{noun}。",
  "{actor} channeled {noun}.": "{actor}生成了{noun}。",
  "{actor} evoked {noun}.": "{actor}激发了{noun}。",
  "{actor} auto-played {noun} via {source}.": "{actor}通过{source}自动打出了{noun}。",
  "{actor} auto-played {noun}.": "{actor}自动打出了{noun}。",

  // turnLedger
  "History detail": "历史详情",
  "net": "精简",
  "full": "完整",
  "Turn {n}": "第{n}回合",
  "TURN {n}": "第{n}回合",
  "−{n} HP": "−{n} 生命",

  // The "Blocked" float (vfx/BLOCKED; the game's Chinese is 格挡).
  "Blocked": "格挡",
  // vfx/POWER_WEARS_OFF (the game's Chinese).
  "Wears Off": "效果结束",
  // FoolQuiver
  "arrows: {ready} ready of {total}": "箭矢：{ready}/{total} 就绪",

  // choiceCheck
  " · within noise": " · 在误差范围内",
  "rollouts to end of act (n={n}): you {you} · AI {ai}": "推演至本幕结束（n={n}）：你 {you} · AI {ai}",
  " · avg HP {you} vs {ai}": " · 平均生命 {you} 对 {ai}",

  // intentBreakdown (scaffolding around the modifier names)
  " (you)": "（你）",
  " (attacker)": "（攻击者）",
  "{n} base": "{n} 基础",
  "capped at {cap} {label}": "上限 {cap}（{label}）",
  "{n} damage per hit": "每次 {n} 点伤害",
  "{n} damage": "{n} 点伤害",

  // cardskin.html (the card-frame test page)
  "Matched against the live deck screen": "与实时牌组界面对照",
  "Cost glyphs — Regent star cost, and inline energy/star runs": "费用图标——储君的辉星费用，以及行内能量/辉星",
  "Pool → frame tint": "卡池 → 卡框色调",
  "Rarity → banner / plaque / portrait border": "稀有度 → 横幅 / 铭牌 / 卡图边框",
  "Type → frame texture, and edge cases": "类型 → 卡框纹理，以及边界情况",
  "Hand states — playable / unaffordable / Bound / Bound and spent": "手牌状态——可打出 / 费用不足 / 魂缚 / 魂缚且已用",
  "Hand size (112px)": "手牌尺寸（112px）",
};
export default zh;
