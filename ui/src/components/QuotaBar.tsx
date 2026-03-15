import { useState } from "react";
import { Progress, Button, Modal, InputNumber, Popconfirm } from "antd";
import { ToolOutlined, SyncOutlined, EditOutlined, DeleteOutlined } from "@ant-design/icons";
import type { QuotaState } from "../types";
import { formatBytes } from "../utils";
import { syncUsage, adjustQuota, resetCycle } from "../api";

interface Props {
  quota: QuotaState;
  onMessage: (text: string, type: "success" | "error" | "info") => void;
}

export default function QuotaBar({ quota, onMessage }: Props) {
  const pct = Math.max(0, Math.min(100, quota.pct));
  const [modalOpen, setModalOpen] = useState(false);
  const [syncGb, setSyncGb] = useState<number | null>(null);
  const [adjustGb, setAdjustGb] = useState<number | null>(null);
  const [syncLoading, setSyncLoading] = useState(false);
  const [adjustLoading, setAdjustLoading] = useState(false);
  const [resetLoading, setResetLoading] = useState(false);

  let strokeColor: string;
  if (quota.pct >= 90) {
    strokeColor = "#ef4444";
  } else if (quota.pct >= 70) {
    strokeColor = "#fbbf24";
  } else {
    strokeColor = "#4ade80";
  }

  const handleSync = async () => {
    if (syncGb == null || syncGb < 0) {
      onMessage("Enter a valid usage value in GB", "error");
      return;
    }
    setSyncLoading(true);
    try {
      const resp = await syncUsage(syncGb);
      if (resp.adjusted_by) {
        onMessage(`Synced: adjusted by ${formatBytes(resp.adjusted_by)}`, "success");
      } else {
        onMessage(resp.note || "Sync complete", "info");
      }
      setSyncGb(null);
    } catch (e) {
      onMessage(`Sync failed: ${e instanceof Error ? e.message : String(e)}`, "error");
    } finally {
      setSyncLoading(false);
    }
  };

  const handleAdjust = async () => {
    if (adjustGb == null) {
      onMessage("Enter a valid GB value", "error");
      return;
    }
    setAdjustLoading(true);
    try {
      const deltaBytes = Math.round(adjustGb * 1073741824);
      await adjustQuota(deltaBytes);
      onMessage(
        `Adjusted by ${adjustGb > 0 ? "+" : ""}${adjustGb.toFixed(2)} GB`,
        "success",
      );
      setAdjustGb(null);
    } catch (e) {
      onMessage(`Adjust failed: ${e instanceof Error ? e.message : String(e)}`, "error");
    } finally {
      setAdjustLoading(false);
    }
  };

  const handleReset = async () => {
    setResetLoading(true);
    try {
      await resetCycle();
      onMessage("Billing cycle reset", "success");
    } catch (e) {
      onMessage(`Reset failed: ${e instanceof Error ? e.message : String(e)}`, "error");
    } finally {
      setResetLoading(false);
    }
  };

  return (
    <>
      <div
        style={{
          background: "#111",
          border: "1px solid #222",
          borderRadius: 8,
          padding: 16,
          height: "100%",
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
          <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
            <span style={{ color: "#666", fontSize: 12, textTransform: "uppercase", letterSpacing: "0.05em" }}>
              Quota Usage
            </span>
            <button
              onClick={() => setModalOpen(true)}
              style={{
                background: "none",
                border: "1px solid #333",
                color: "#666",
                borderRadius: 4,
                cursor: "pointer",
                padding: "2px 6px",
                fontSize: 12,
                display: "flex",
                alignItems: "center",
                minHeight: 24,
                minWidth: 24,
              }}
              title="Adjust quota"
            >
              <ToolOutlined />
            </button>
          </div>
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
        <div style={{ display: "flex", justifyContent: "space-between", marginTop: 6, fontSize: 11, color: "#555" }}>
          <span>Down: {formatBytes(quota.used_download)} / Up: {formatBytes(quota.used_upload)}</span>
          <span>{quota.billing_month}</span>
        </div>
      </div>

      <Modal
        title="Quota Adjustments"
        open={modalOpen}
        onCancel={() => setModalOpen(false)}
        footer={null}
        width={400}
        styles={{
          content: { background: "#111", border: "1px solid #222" },
          header: { background: "#111" },
        }}
      >
        <div style={{ display: "flex", flexDirection: "column", gap: 20, paddingTop: 8 }}>
          <div>
            <div style={{ color: "#fff", fontSize: 14, fontWeight: 500, marginBottom: 4 }}>
              <SyncOutlined style={{ marginRight: 6 }} />
              Sync with Starlink
            </div>
            <div style={{ color: "#555", fontSize: 12, marginBottom: 8 }}>
              Enter usage from the Starlink app to reconcile.
            </div>
            <div style={{ display: "flex", gap: 8 }}>
              <InputNumber
                value={syncGb}
                onChange={setSyncGb}
                placeholder="GB used"
                min={0}
                step={0.01}
                style={{ flex: 1 }}
                size="large"
              />
              <Button
                type="primary"
                onClick={handleSync}
                loading={syncLoading}
                size="large"
                style={{ minWidth: 44 }}
              >
                Sync
              </Button>
            </div>
          </div>

          <div style={{ borderTop: "1px solid #222" }} />

          <div>
            <div style={{ color: "#fff", fontSize: 14, fontWeight: 500, marginBottom: 4 }}>
              <EditOutlined style={{ marginRight: 6 }} />
              Manual Adjust
            </div>
            <div style={{ color: "#555", fontSize: 12, marginBottom: 8 }}>
              Add or subtract from the usage counter.
            </div>
            <div style={{ display: "flex", gap: 8 }}>
              <InputNumber
                value={adjustGb}
                onChange={setAdjustGb}
                placeholder="GB (+/-)"
                step={0.01}
                style={{ flex: 1 }}
                size="large"
              />
              <Button
                onClick={handleAdjust}
                loading={adjustLoading}
                size="large"
                style={{ minWidth: 44, borderColor: "#fbbf24", color: "#fbbf24" }}
              >
                Adjust
              </Button>
            </div>
          </div>

          <div style={{ borderTop: "1px solid #222" }} />

          <div>
            <div style={{ color: "#fff", fontSize: 14, fontWeight: 500, marginBottom: 4 }}>
              <DeleteOutlined style={{ marginRight: 6 }} />
              Reset Billing Cycle
            </div>
            <div style={{ color: "#555", fontSize: 12, marginBottom: 8 }}>
              Zero out usage and start a fresh billing cycle.
            </div>
            <Popconfirm
              title="Reset billing cycle?"
              description="This will zero out all usage counters."
              onConfirm={handleReset}
              okText="Reset"
              okButtonProps={{ danger: true }}
            >
              <Button
                danger
                loading={resetLoading}
                size="large"
                style={{ minWidth: 44 }}
              >
                Reset Cycle
              </Button>
            </Popconfirm>
          </div>
        </div>
      </Modal>
    </>
  );
}
