import { useEffect, useState } from "react";
import { getHealth, API_BASE_URL } from "./lib/api";

type ApiState =
  | { kind: "loading" }
  | { kind: "ok"; status: string }
  | { kind: "error"; message: string };

function App() {
  const [api, setApi] = useState<ApiState>({ kind: "loading" });

  useEffect(() => {
    let cancelled = false;
    getHealth()
      .then((h) => !cancelled && setApi({ kind: "ok", status: h.status }))
      .catch(
        (e) => !cancelled && setApi({ kind: "error", message: String(e) }),
      );
    return () => {
      cancelled = true;
    };
  }, []);

  return;
}

function ApiBadge({ api }: { api: ApiState }) {
  if (api.kind === "loading") {
    return <span className="text-amber-400">checking…</span>;
  }
  if (api.kind === "ok") {
    return <span className="text-emerald-400">● connected ({api.status})</span>;
  }
  return (
    <span className="text-rose-400" title={api.message}>
      ● unreachable (start the API)
    </span>
  );
}

export default App;
