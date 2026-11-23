// The Mode bar's vocabulary and its status in words (MentorBar.tsx draws it).

import { t } from "../i18n";

export type MentorMode = "off" | "coach" | "hints" | "aiplays";
export type Brain = "search" | "instinct";

/** The dot beside the status words. */
export type MentorState = "idle" | "thinking" | "ai" | "off";

/**
 * The words for the status: what the AI is doing for you right now, in a
 * player's words (the mod's MentorBadge). Pure, for the tests.
 */
export function mentorStatus(s: {
  mode: MentorMode;
  aiPlaying: boolean;
  thinking: boolean;
  brain: Brain | null;
  autobattle: boolean;
  /** The step a whole-run autoplay is on, for "AI playing · step 12". */
  aiStep?: number;
}): { text: string; state: MentorState } {
  const who = s.brain === "instinct" ? t("Instinct") : t("Search");
  if (s.aiPlaying) {
    return { text: s.aiStep ? t("AI playing · step {n}", { n: s.aiStep }) : t("AI playing"), state: "ai" };
  }
  if (s.mode === "off") return { text: t("AI off"), state: "off" };
  if (s.thinking) return { text: t("{who} thinking…", { who }), state: "thinking" };
  if (s.autobattle) return { text: t("Autobattle: the AI fights, you decide the run"), state: "idle" };
  return s.mode === "coach"
    ? { text: t("Coach: your call — P to peek"), state: "idle" }
    : { text: t("{who} ready", { who }), state: "idle" };
}
