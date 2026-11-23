/** Game names and rules text the page writes itself (the wire does not carry
 *  them): ascension levels, game modes, dominions, characters, acts, intents,
 *  Neow's epithet. Every Chinese string is the game's OWN text, copied from
 *  `sts2-pycli/localization_zhs/` (the table and key are noted per group), so
 *  the page reads as the game does. Keys are the English the page writes. */
const table: Record<string, string> = {
  // ascension.json LEVEL_NN.title / .description
  "No Ascension": "无进阶",
  "Play without any Ascension modifiers.": "在没有任何进阶调整的影响下游玩。",
  "Swarming Elites": "精英蜂拥",
  "[gold]Elites[/gold] spawn more often.": "[gold]精英[/gold]敌人出现更加频繁。",
  "Weary Traveler": "旅途劳顿",
  "Ancients only heal [blue]80%[/blue] of your missing HP.": "先古之民只会回复你已损失生命值的[blue]80%[/blue]。",
  "Poverty": "贫穷",
  "Enemies and Treasure Chests drop [blue]25%[/blue] less [gold]Gold[/gold].": "敌人和宝箱掉落的[gold]金币[/gold]减少[blue]25%[/blue]。",
  "Tight Belt": "收紧腰带",
  "Start each run with 1 less potion slot.": "游戏开始时药水栏位减少[blue]1[/blue]。",
  "Ascender's Bane": "进阶之灾",
  "Start each run [red]Cursed[/red].": "游戏开始时被[red]诅咒[/red]。",
  "Inflation": "通货膨胀",
  "Removing cards from your deck at the Merchant is more expensive.": "商人的卡牌移除服务价格提升。",
  "Scarcity": "稀缺",
  "Rare and Upgraded cards appear less often.": "稀有和已升级卡牌更少出现。",
  "Tough Enemies": "强韧敌人",
  "All enemies are harder to kill.": "所有的敌人变得更难杀死。",
  "Deadly Enemies": "致命敌人",
  "All enemies have deadlier attacks.": "所有的敌人变得更加致命。",
  "Double Boss": "双重Boss",
  "Fight two bosses at the end of Act 3.": "在[blue]第3[/blue][gold]阶段[/gold]最后与[blue]2[/blue]个[red]Boss[/red]战斗。",

  // Game modes: main_menu_ui.json DREAD.title / BLITZ.title, ascension.json
  // PORTRAIT_GAME_MODE.* ("Standard" is game_modes.json SINGLE_PLAYER_STANDARD.title).
  "Standard": "标准",
  "Dread": "恐惧",
  "Blitz": "极速",
  "Blitz needs the Rust engine.": "极速模式需要 Rust 引擎。",
  "Blitz needs game build {v}.": "极速模式需要游戏版本 {v}。",
  "Blitz ascension 0 · no modifiers": "极速进阶 0 · 无进阶调整",
  "Blitz A{n}": "极速 A{n}",
  "Dread, {dominion}": "恐惧，{dominion}",
  // ascension.json DREAD.title ("{BossName}的支配")
  "{boss}'s Dominion": "{boss}的支配",
  // encounters.json <BOSS>_BOSS.title (v0.113.0 bosses with a dominion)
  "Aeonglass": "永世沙漏",
  "Ceremonial Beast": "仪式兽",
  "The Insatiable": "无厌沙虫",
  "Kaiser Crab": "帝皇蟹",
  "The Kin": "同族小队",
  "Knowledge Demon": "知识恶魔",
  "Lagavulin Matriarch": "乐加维林族母",
  "Queen": "女王",
  "Soul Fysh": "灵魂异鱼",
  "Test Subject": "实验体",
  "Vantom": "墨影幻灵",
  "Waterfall Giant": "瀑布巨兽",
  // merchant_room.json MERCHANT.blitzSelection.none / .remaining
  "[red]You can't choose any more items.[/red]": "[red]你无法再选择更多货物了。[/red]",
  "You may choose [blue]{n}[/blue] more item.": "你还可以选择[blue]{n}[/blue]件货物。",
  "You may choose [blue]{n}[/blue] more items.": "你还可以选择[blue]{n}[/blue]件货物。",

  // characters.json <ID>.title (without the English "The ")
  "Ironclad": "铁甲战士",
  "Silent": "静默猎手",
  "Defect": "故障机器人",
  "Regent": "储君",
  "Necrobinder": "亡灵契约师",
  "Fool": "愚者",

  // acts.json <ACT>.title
  "Overgrowth": "密林",
  "Underdocks": "暗港",
  "Hive": "巢穴",
  "Glory": "荣耀",

  // ancients.json NEOW.epithet
  "Mother of Resurrection (Exiled)": "复生之母（被放逐）",

  // intents.json <INTENT>.title / .description, BBCode stripped ("Hidden"
  // has no tip in the game; its text is the page's own).
  "Aggressive": "攻势",
  "Empower": "强化",
  "Strategic": "策略",
  "Defensive": "守势",
  "Cowardly": "懦弱",
  "Heal": "回复",
  "Hidden": "隐藏",
  "Summon": "召唤",
  "Sleeping": "沉睡",
  "Stunned": "击晕",
  "Malicious": "恶意",
  "Death Blow": "濒死一击",
  "Unknown": "未知",
  "This enemy intends to Attack.": "这个敌人将要攻击。",
  "This enemy intends to Attack for {d} damage.": "这个敌人将要攻击造成{d}点伤害。",
  "This enemy intends to Attack for {d} damage {hits} times.": "这个敌人将要攻击造成{d}点伤害{hits}次。",
  "This enemy intends to use a Buff.": "这个敌人将要使用一个强化效果。",
  "This enemy intends to apply a Debuff to you.": "这个敌人将要对你施加一个负面效果。",
  "This enemy intends to apply a powerful Debuff to you.": "这个敌人将要对你施加一个强大的负面效果。",
  "This enemy intends to Block on its turn.": "这个敌人将会在其回合获得格挡。",
  "This enemy intends to Escape.": "这个敌人将会逃跑。",
  "This enemy intends to Heal.": "这个敌人将会回复。",
  "This enemy's intention is hidden.": "这个敌人的意图被隐藏了。",
  "This enemy intends to summon Monsters.": "这个敌人将要召唤怪物。",
  "This enemy is doing nothing this turn.": "这个敌人本回合不会采取任何行动。",
  "This enemy can't act on its next turn.": "这个敌人在其下一回合无法行动。",
  "This enemy intends to give you Status cards.": "这个敌人将会给你状态牌。",
  "This enemy intends to apply Afflictions on your cards.": "这个敌人将要对你的卡牌施加侵蚀。",
  "This creature is trying to take you down with it before being destroyed.": "这个生物试图在被摧毁前拉你一起陪葬。",
  "This creature is trying to take you down with it. It will attack you for {d} damage before being destroyed.": "这个生物试图拉你一起陪葬。它会在被摧毁前攻击你造成{d}点伤害。",
  "This enemy's intentions are Unknown.": "这个敌人将要进行某种未知行动。",

  // Enemy label fallback (no name on the wire).
  "enemy {n}": "敌人 {n}",
  // The glossary's spelled-out energy run (`fillTemplate`), as the game's
  // Chinese text spells it ("获得2点能量").
  "{n} Energy": "{n}点能量",
  // Runtime keys: archetype labels (web/public/archetypes_*.json), shown on
  // card-reward chips and the deck-analysis membership panel.
  "Exhaust": "消耗",
  "Mixed": "混合",
  "Self-damage": "自伤",
  "Strike": "打击",
  "Vulnerable": "易伤",
};

export default table;
