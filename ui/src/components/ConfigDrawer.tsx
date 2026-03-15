import { useEffect, useState } from "react";
import { Drawer, Form, InputNumber, Input, Button, Divider, Spin } from "antd";
import type { ConfigValues } from "../types";
import { getConfig, updateConfig } from "../api";

interface Props {
  open: boolean;
  onClose: () => void;
  onSaved: (config: ConfigValues) => void;
}

export default function ConfigDrawer({ open, onClose, onSaved }: Props) {
  const [form] = Form.useForm();
  const [loading, setLoading] = useState(false);
  const [saving, setSaving] = useState(false);

  useEffect(() => {
    if (!open) return;
    setLoading(true);
    getConfig()
      .then((cfg) => {
        form.setFieldsValue(cfg);
      })
      .finally(() => setLoading(false));
  }, [open, form]);

  const handleSave = async () => {
    setSaving(true);
    try {
      const values = form.getFieldsValue();
      const cfg = await updateConfig(values);
      onSaved(cfg);
      onClose();
    } catch (e) {
      // Form-level error display via antd message is handled by parent
      throw e;
    } finally {
      setSaving(false);
    }
  };

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
            Quota &amp; Billing
          </Divider>
          <Form.Item label="Monthly Quota (GB)" name="monthly_quota_gb">
            <InputNumber min={1} max={500} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item label="Billing Reset Day" name="billing_reset_day">
            <InputNumber min={1} max={28} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item label="Plan Cost ($/month)" name="plan_cost_monthly">
            <InputNumber min={0} step={0.01} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item label="Overage Cost ($/GB)" name="overage_cost_per_gb">
            <InputNumber min={0} step={0.01} style={{ width: "100%" }} />
          </Form.Item>

          <Divider orientation="left" style={{ color: "#666", fontSize: 12 }}>
            Rate Curve
          </Divider>
          <Form.Item label="Max Rate (kbit/s)" name="max_rate_kbit">
            <InputNumber min={1} max={500000} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item label="Min Rate (kbit/s)" name="min_rate_kbit">
            <InputNumber min={64} max={50000} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item
            label="Curve Shape"
            name="curve_shape"
            extra="0.10 - 2.00 (lower = more aggressive throttle)"
          >
            <InputNumber
              min={0.1}
              max={2.0}
              step={0.01}
              style={{ width: "100%" }}
            />
          </Form.Item>
          <Form.Item
            label="Down/Up Ratio"
            name="down_up_ratio"
            extra="0.50 - 0.95 (portion allocated to download)"
          >
            <InputNumber
              min={0.5}
              max={0.95}
              step={0.01}
              style={{ width: "100%" }}
            />
          </Form.Item>

          <Divider orientation="left" style={{ color: "#666", fontSize: 12 }}>
            Bucket / Burst
          </Divider>
          <Form.Item label="Bucket Duration (sec)" name="bucket_duration_sec">
            <InputNumber min={30} max={900} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item
            label="Burst Drain Ratio"
            name="burst_drain_ratio"
            extra="0.01 - 0.50"
          >
            <InputNumber
              min={0.01}
              max={0.5}
              step={0.01}
              style={{ width: "100%" }}
            />
          </Form.Item>

          <Divider orientation="left" style={{ color: "#666", fontSize: 12 }}>
            Network
          </Divider>
          <Form.Item label="WAN Interface" name="wan_iface">
            <Input />
          </Form.Item>
          <Form.Item label="LAN Interface" name="lan_iface">
            <Input />
          </Form.Item>
          <Form.Item label="IFB Interface" name="ifb_iface">
            <Input />
          </Form.Item>
          <Form.Item label="Dish Address" name="dish_addr">
            <Input />
          </Form.Item>

          <Divider orientation="left" style={{ color: "#666", fontSize: 12 }}>
            Intervals
          </Divider>
          <Form.Item label="Tick Interval (sec)" name="tick_interval_sec">
            <InputNumber min={1} max={10} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item label="Save Interval (sec)" name="save_interval_sec">
            <InputNumber min={10} max={600} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item
            label="Device Scan Interval (sec)"
            name="device_scan_interval_sec"
          >
            <InputNumber min={5} max={120} style={{ width: "100%" }} />
          </Form.Item>
          <Form.Item
            label="Dish Poll Interval (sec)"
            name="dish_poll_interval_sec"
          >
            <InputNumber min={5} max={300} style={{ width: "100%" }} />
          </Form.Item>
        </Form>
      )}
    </Drawer>
  );
}
