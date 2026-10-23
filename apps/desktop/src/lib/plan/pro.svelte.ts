// The plan this copy is on, for every part of the window to check, and the
// prompt shown when something needs Pro (lib/UpgradeDialog.svelte).
import { api, onPlanChanged, type Plan } from "../api";

export type ProFeature = "accounts" | "calendarSync" | "rules" | "sendLater" | "snooze" | "templates" | "signatures";

/** What each feature is called in the prompt. */
export const featureNames: Record<ProFeature, string> = {
  accounts: "More than two accounts",
  calendarSync: "Google and Outlook calendar sync",
  rules: "Rules",
  sendLater: "Send later",
  snooze: "Snooze",
  templates: "Templates",
  signatures: "More than one signature per account",
};

export const pro = $state<{ plan: Plan | null; asking: ProFeature | null }>({ plan: null, asking: null });

async function load() {
  try {
    pro.plan = await api.getPlan();
  } catch {
    // Unknown until the next change; nothing is blocked meanwhile.
  }
}

/** Loads the plan and follows it from now on. */
export function watchPlan() {
  load();
  onPlanChanged(load).catch(() => {});
}

/** Whether Pro features can be used. While the plan is loading they can;
 *  the app checks again before doing anything. */
export function hasPro(): boolean {
  return pro.plan?.tier !== "free";
}

/** True when `feature` can be used; otherwise shows the upgrade prompt. */
export function needPro(feature: ProFeature): boolean {
  if (hasPro()) return true;
  pro.asking = feature;
  return false;
}

/** The plan as one short line, such as "Pro trial, 9 days left". */
export function planLine(plan: Plan): string {
  switch (plan.reason) {
    case "unlocked":
      return "Every feature is included in this copy.";
    case "license":
      return "Novero Mail Pro";
    case "trial":
      return `Pro trial, ${plan.trialDaysLeft} ${plan.trialDaysLeft === 1 ? "day" : "days"} left`;
    case "unchecked":
      return "Free: your license needs checking";
    case "ended":
      return "Free: your license is no longer active";
    case "tooNew":
      return "Free: this version is newer than your license covers";
    default:
      return "Novero Mail Free";
  }
}

/** Whether an update came out after the license's year of updates. */
export function outsideLicense(plan: Plan | null, released: number | null): boolean {
  const until = plan?.license?.updatesUntil;
  return plan?.reason === "license" && until != null && released != null && released > until;
}
