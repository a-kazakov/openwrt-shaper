import { Progress } from "antd";
import type { QuotaState } from "../types";
import { formatBytes } from "../utils";

interface Props {
  quota: QuotaState;
}

export default function QuotaBar({ quota }: Props) {
  const pct = Math.max(0, Math.min(100, quota.pct));

  let strokeColor: string;
  if (quota.pct >= 90) {
    strokeColor = "#ef4444";
  } else if (quota.pct >= 70) {
    strokeColor = "#fbbf24";
  } else {
    strokeColor = "#4ade80";
  }

  return (
    <div
      style={{
        background: "#111",
        border: "1px solid #222",
        borderRadius: 8,
        padding: 16,
        marginTop: 12,
      }}
    >
      <div
        style={{
          display: "flex",
          justifyContent: "space-between",
          alignItems: "center",
          marginBottom: 8,
        }}
      >
        <span style={{ color: "#666", fontSize: 12, textTransform: "uppercase", letterSpacing: "0.05em" }}>
          Quota Usage
        </span>
        <span style={{ color: "#999", fontSize: 13 }}>
          {quota.pct}% used ({formatBytes(quota.used)} / {formatBytes(quota.total)})
        </span>
      </div>
      <Progress
        percent={pct}
        showInfo={false}
        strokeColor={strokeColor}
        trailColor="#222"
        size={["100%", 10]}
      />
    </div>
  );
}
