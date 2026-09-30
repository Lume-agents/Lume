import type { SystemBannerTone } from "./SystemBannerStack.svelte";

export const SYSTEM_BANNER_CONTEXT = Symbol("lume-system-banners");

export type SystemBannerNotice = {
  id: string;
  message: string;
  tone?: SystemBannerTone;
  onDismiss?: () => void;
};

export type SystemBannerReporter = (notice: SystemBannerNotice) => void;

/** Do not keep re-announcing the same background failure on each refresh. */
export function createSystemBannerSource(report: SystemBannerReporter | undefined) {
  let previous = new Map<string, string>();
  return (notices: SystemBannerNotice[]) => {
    if (!report) return;
    const next = new Map<string, string>();
    for (const notice of notices) {
      const key = `${notice.tone ?? "info"}:${notice.message}`;
      next.set(notice.id, key);
      if (previous.get(notice.id) !== key) report(notice);
    }
    previous = next;
  };
}
