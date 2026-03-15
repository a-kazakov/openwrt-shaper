import { useEffect, useState, useCallback, useRef } from "react";
import { Drawer, Form, InputNumber, Input, Button, Slider, Spin, Divider } from "antd";
import type { ConfigValues } from "../types";
import { getConfig, updateConfig } from "../api";

interface Props {
  open: boolean;
  onClose: () => void;
  onSaved: (config: ConfigValues) => void;
}

/** Mini curve preview canvas. */
function CurvePreview({
  shape,
  maxKbit,
  minKbit,
}: {
  shape: number;
  maxKbit: number;
  minKbit: number;
}) {
  const canvasRef = useRef<HTMLCanvasElement>(null);

  const draw = useCallback(() => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const dpr = window.devicePixelRatio || 1;
    const w = rect.width;
    const h = rect.height;
    canvas.width = w * dpr;
    canvas.height = h * dpr;
    const ctx = canvas.getContext("2d");
    if (!ctx) return;
    ctx.scale(dpr, dpr);
    ctx.clearRect(0, 0, w, h);

    const pad = 2;
    const pw = w - pad * 2;
    const ph = h - pad * 2;

    // Curve area
    ctx.beginPath();
    const steps = 100;
    for (let i = 0; i <= steps; i++) {
      const ratio = i / steps;
      const curved = Math.pow(ratio, shape);
      const rate = minKbit + (maxKbit - minKbit) * curved;
      const x = pad + (1 - ratio) * pw;
      const y = pad + ph - (rate / maxKbit) * ph;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.lineTo(pad, pad + ph);
    ctx.lineTo(pad + pw, pad + ph);
    ctx.closePath();
    ctx.fillStyle = "rgba(255,255,255,0.06)";
    ctx.fill();

    // Curve line
    ctx.beginPath();
    for (let i = 0; i <= steps; i++) {
      const ratio = i / steps;
      const curved = Math.pow(ratio, shape);
      const rate = minKbit + (maxKbit - minKbit) * curved;
      const x = pad + (1 - ratio) * pw;
      const y = pad + ph - (rate / maxKbit) * ph;
      if (i === 0) ctx.moveTo(x, y);
      else ctx.lineTo(x, y);
    }
    ctx.strokeStyle = "rgba(255,255,255,0.5)";
    ctx.lineWidth = 2;
    ctx.stroke();
  }, [shape, maxKbit, minKbit]);

  useEffect(() => {
    draw();
    const canvas = canvasRef.current;
    if (!canvas) return;
    const observer = new ResizeObserver(() => draw());
    observer.observe(canvas);
    return () => observer.disconnect();
  }, [draw]);

  return (
    <canvas
      ref={canvasRef}
      style={{
        width: "100%",
        height: 60,
        display: "block",
        borderRadius: 4,
        background: "#0a0a0a",
      }}
    />
  );
}

const derivStyle: React.CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  fontSize: 12,
  color: "#888",
  padding: "4px 0",
};

export default function ConfigDrawer({ open, onClose, onSaved }: Props) {
  const [form] = Form.useForm();
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);
  const [showAdvanced, setShowAdvanced] = useState(false);

  const [maxKbit, setMaxKbit] = useState(50000);
  const [minKbit, setMinKbit] = useState(1000);
  const [curveShape, setCurveShape] = useState(0.4);
  const [quotaGb, setQuotaGb] = useState(20);

  useEffect(() => {
    if (!open) return;
    setLoading(true);
    getConfig()
      .then((cfg) => {
        form.setFieldsValue({
          ...cfg,
          max_rate_mbit: cfg.max_rate_kbit / 1000,
          min_rate_mbit: cfg.min_rate_kbit / 1000,
        });
        setMaxKbit(cfg.max_rate_kbit);
        setMinKbit(cfg.min_rate_kbit);
        setCurveShape(cfg.curve_shape);
        setQuotaGb(cfg.monthly_quota_gb);
      })
      .finally(() => setLoading(false));
  }, [open, form]);

  const handleSave = async () => {
    setSaving(true);
    try {
      const values = form.getFieldsValue();
      const payload: Partial<ConfigValues> = { ...values };
      if (values.max_rate_mbit != null) {
        payload.max_rate_kbit = Math.round(values.max_rate_mbit * 1000);
      }
      if (values.min_rate_mbit != null) {
        payload.min_rate_kbit = Math.round(values.min_rate_mbit * 1000);
      }
      delete (payload as Record<string, unknown>).max_rate_mbit;
      delete (payload as Record<string, unknown>).min_rate_mbit;
      const cfg = await updateConfig(payload);
      onSaved(cfg);
      onClose();
    } catch {
      // error handled by parent message
    } finally {
      setSaving(false);
    }
  };

  const maxBytesPerSec = (maxKbit * 1000) / 8;
  const quotaBytes = quotaGb * 1073741824;
  const burnHours = maxBytesPerSec > 0 ? quotaBytes / maxBytesPerSec / 3600 : 0;
  const burnLabel =
    burnHours < 1
      ? `${Math.round(burnHours * 60)}m`
      : burnHours < 24
        ? `${burnHours.toFixed(1)}h`
        : `${(burnHours / 24).toFixed(1)}d`;

  const minBytesPerSec = (minKbit * 1000) / 8;
  const gbPerHrAtMin = (minBytesPerSec * 3600) / 1073741824;

  return (
    <Drawer
      title="Configuration"
      placement="right"
      width={Math.min(420, window.innerWidth)}
      onClose={onClose}
      open={open}
      styles={{
        body: { paddingBottom: 80, background: "#111" },
        header: { background: "#111", borderBottom: "1px solid #222" },
      }}
      footer={
        <div style={{ display: "flex", gap: 8, justifyContent: "flex-end" }}>
          <Button onClick={onClose} size="large" style={{ minHeight: 44 }}>
            Cancel
          </Button>
          <Button
            type="primary"
            onClick={handleSave}
            loading={saving}
            size="large"
            style={{ minHeight: 44 }}
          >
            Save
          </Button>
        </div>
      }
    >
      {loading ? (
        <div style={{ textAlign: "center", padding: 40 }}>
          <Spin />
        </div>
      ) : (
        <Form form={form} layout="vertical" size="large">
          <Divider orientation="left" style={{ color: "#666", fontSize: 12 }}>
            Quota
          </Divider>
          <Form.Item
            label="Monthly Quota (GB)"
            name="monthly_quota_gb"
            tooltip="Total data allowance per billing cycle. The shaper maps remaining quota to bandwidth using the rate curve."
          >
            <InputNumber
              min={1}
              max={500}
              style={{ width: "100%" }}
              onChange={(v) => v != null && setQuotaGb(v as number)}
            />
          </Form.Item>
          <Form.Item
            label="Billing Reset Day"
            name="billing_reset_day"
            tooltip="Day of the month when usage resets to zero and a new billing cycle starts."
          >
            <InputNumber min={1} max={28} style={{ width: "100%" }} />
          </Form.Item>

          <Divider orientation="left" style={{ color: "#666", fontSize: 12 }}>
            Rate Curve
          </Divider>

          <Form.Item
            label={`Max Rate: ${maxKbit / 1000} Mbps`}
            name="max_rate_mbit"
            tooltip="Speed limit when quota is nearly full. This is the ceiling at the start of the billing cycle."
          >
            <Slider
              min={1}
              max={100}
              step={1}
              onChange={(v) => setMaxKbit(v * 1000)}
              tooltip={{ formatter: (v) => `${v} Mbps` }}
            />
          </Form.Item>

          <Form.Item
            label={`Min Rate: ${(minKbit / 1000).toFixed(1)} Mbps`}
            name="min_rate_mbit"
            tooltip="Floor speed when quota is nearly exhausted. Traffic will never be shaped below this rate."
          >
            <Slider
              min={0.1}
              max={10}
              step={0.1}
              onChange={(v) => setMinKbit(v * 1000)}
              tooltip={{ formatter: (v) => `${v} Mbps` }}
            />
          </Form.Item>

          <Form.Item
            label={`Curve Shape: ${curveShape.toFixed(2)}`}
            name="curve_shape"
            tooltip="Controls how aggressively speed drops as quota depletes. Lower values throttle earlier; higher values maintain speed longer before a steep drop."
          >
            <Slider
              min={0.1}
              max={2.0}
              step={0.01}
              onChange={(v) => setCurveShape(v)}
              tooltip={{ formatter: (v) => `${v?.toFixed(2)}` }}
            />
          </Form.Item>

          <CurvePreview shape={curveShape} maxKbit={maxKbit} minKbit={minKbit} />

          <div style={{ marginTop: 12, marginBottom: 8 }}>
            <div style={derivStyle}>
              <span>Burn {quotaGb} GB at max</span>
              <span style={{ color: "#fff" }}>{burnLabel}</span>
            </div>
            <div style={derivStyle}>
              <span>Speed at min rate</span>
              <span style={{ color: "#fff" }}>{gbPerHrAtMin.toFixed(2)} GB/hr</span>
            </div>
          </div>

          <Form.Item
            label="Down/Up Ratio"
            name="down_up_ratio"
            tooltip="How the sustained rate is split between download and upload. 0.80 = 80% download, 20% upload."
          >
            <InputNumber
              min={0.5}
              max={0.95}
              step={0.01}
              style={{ width: "100%" }}
            />
          </Form.Item>

          <div style={{ marginTop: 16 }}>
            <Button
              type="text"
              onClick={() => setShowAdvanced(!showAdvanced)}
              style={{ color: "#666", padding: 0, fontSize: 13 }}
            >
              {showAdvanced ? "Hide" : "Show"} Advanced
            </Button>
          </div>

          {showAdvanced && (
            <>
              <Divider orientation="left" style={{ color: "#666", fontSize: 12 }}>
                Bucket / Burst
              </Divider>
              <Form.Item
                label="Bucket Duration (sec)"
                name="bucket_duration_sec"
                tooltip="How many seconds of curve-rate bandwidth each device can store. Longer = bigger burst buffer before throttling kicks in."
              >
                <InputNumber min={30} max={900} style={{ width: "100%" }} />
              </Form.Item>
              <Form.Item
                label="Burst Drain Ratio"
                name="burst_drain_ratio"
                tooltip="Fraction of the bucket drained per tick to set the burst ceiling. Higher = faster bursts but shorter burst duration."
              >
                <InputNumber min={0.01} max={0.5} step={0.01} style={{ width: "100%" }} />
              </Form.Item>

              <Divider orientation="left" style={{ color: "#666", fontSize: 12 }}>
                Network
              </Divider>
              <Form.Item
                label="WAN Interface"
                name="wan_iface"
                tooltip="Upstream-facing network interface. Set to 'auto' to detect from the default route."
              >
                <Input />
              </Form.Item>
              <Form.Item
                label="LAN Interface"
                name="lan_iface"
                tooltip="LAN bridge interface where download shaping is applied. Set to 'auto' to detect."
              >
                <Input />
              </Form.Item>
              <Form.Item
                label="Dish Address"
                name="dish_addr"
                tooltip="Starlink dish gRPC address for status polling (host:port)."
              >
                <Input />
              </Form.Item>

              <Divider orientation="left" style={{ color: "#666", fontSize: 12 }}>
                Intervals
              </Divider>
              <Form.Item
                label="Tick Interval (sec)"
                name="tick_interval_sec"
                tooltip="How often the engine reads counters, refills buckets, and updates shaping rules."
              >
                <InputNumber min={1} max={10} style={{ width: "100%" }} />
              </Form.Item>
              <Form.Item
                label="Save Interval (sec)"
                name="save_interval_sec"
                tooltip="How often quota state is persisted to disk. Lower = less data loss on crash, higher = less disk wear."
              >
                <InputNumber min={10} max={600} style={{ width: "100%" }} />
              </Form.Item>
              <Form.Item
                label="Device Scan Interval (sec)"
                name="device_scan_interval_sec"
                tooltip="How often ARP/DHCP tables are scanned for new devices joining the network."
              >
                <InputNumber min={5} max={120} style={{ width: "100%" }} />
              </Form.Item>
              <Form.Item
                label="Dish Poll Interval (sec)"
                name="dish_poll_interval_sec"
                tooltip="How often the Starlink dish is polled for status (latency, signal, obstruction)."
              >
                <InputNumber min={5} max={300} style={{ width: "100%" }} />
              </Form.Item>
            </>
          )}
        </Form>
      )}
    </Drawer>
  );
}
