import { useEffect } from "react";
import { api, useSnapshot } from "./lib/api";
import Popover from "./components/Popover";
import Settings from "./components/Settings";

export default function App({ windowLabel }: { windowLabel: string }) {
  const snapshot = useSnapshot();

  // Escape closes the popover, like a native menu.
  useEffect(() => {
    if (windowLabel !== "main") return;
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") api.hidePopover();
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [windowLabel]);

  if (windowLabel === "settings") return <Settings snapshot={snapshot} />;
  return <Popover snapshot={snapshot} />;
}
