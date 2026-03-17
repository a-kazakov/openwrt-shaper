import { Alert } from "antd";
import type { Warning } from "../types";

interface Props {
  warnings: Warning[];
}

export default function WarningBanner({ warnings }: Props) {
  if (warnings.length === 0) return null;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, marginBottom: 12 }}>
      {warnings.map((w) => (
        <Alert
          key={w.id}
          message={w.message}
          type={w.level === "error" ? "error" : "warning"}
          showIcon
          style={{
            background: w.level === "error" ? "rgba(239,68,68,0.1)" : "rgba(251,191,36,0.1)",
            border: `1px solid ${w.level === "error" ? "#ef4444" : "#fbbf24"}`,
          }}
        />
      ))}
    </div>
  );
}
