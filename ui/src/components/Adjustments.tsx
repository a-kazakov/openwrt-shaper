import { useState } from "react";
import { Card, InputNumber, Button, Row, Col, Popconfirm } from "antd";
import { SyncOutlined, EditOutlined, DeleteOutlined } from "@ant-design/icons";
import { syncUsage, adjustQuota, resetCycle } from "../api";
import { formatBytes } from "../utils";

interface Props {
  onMessage: (text: string, type: "success" | "error" | "info") => void;
}

const cardStyle: React.CSSProperties = {
  background: "#111",
  borderColor: "#222",
  height: "100%",
};

const titleStyle: React.CSSProperties = {
  color: "#fff",
  fontSize: 15,
  fontWeight: 500,
  marginBottom: 6,
};

const descStyle: React.CSSProperties = {
  color: "#555",
  fontSize: 12,
  marginBottom: 14,
  lineHeight: 1.4,
};

export default function Adjustments({ onMessage }: Props) {
  const [syncGb, setSyncGb] = useState<number | null>(null);
  const [adjustGb, setAdjustGb] = useState<number | null>(null);
  const [syncLoading, setSyncLoading] = useState(false);
  const [adjustLoading, setAdjustLoading] = useState(false);
  const [resetLoading, setResetLoading] = useState(false);

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
    <div style={{ marginTop: 12 }}>
      <div
        style={{
          color: "#666",
          fontSize: 12,
          textTransform: "uppercase",
          letterSpacing: "0.05em",
          marginBottom: 8,
        }}
      >
        Adjustments
      </div>
      <Row gutter={[10, 10]}>
        <Col xs={24} md={8}>
          <Card style={cardStyle} styles={{ body: { padding: 16 } }}>
            <div style={titleStyle}>
              <SyncOutlined style={{ marginRight: 8 }} />
              Sync with Starlink
            </div>
            <div style={descStyle}>
              Enter the usage shown in the Starlink app to reconcile.
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
          </Card>
        </Col>
        <Col xs={24} md={8}>
          <Card style={cardStyle} styles={{ body: { padding: 16 } }}>
            <div style={titleStyle}>
              <EditOutlined style={{ marginRight: 8 }} />
              Manual Adjust
            </div>
            <div style={descStyle}>
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
                style={{
                  minWidth: 44,
                  borderColor: "#fbbf24",
                  color: "#fbbf24",
                }}
              >
                Adjust
              </Button>
            </div>
          </Card>
        </Col>
        <Col xs={24} md={8}>
          <Card style={cardStyle} styles={{ body: { padding: 16 } }}>
            <div style={titleStyle}>
              <DeleteOutlined style={{ marginRight: 8 }} />
              Reset Billing Cycle
            </div>
            <div style={descStyle}>
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
          </Card>
        </Col>
      </Row>
    </div>
  );
}
