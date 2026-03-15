import { Card, Row, Col } from "antd";
import type { QuotaState, CurveState, ThroughputState } from "../types";
import { formatBytes, formatRate, formatRateKbit } from "../utils";

interface Props {
  quota: QuotaState;
  curve: CurveState;
  throughput: ThroughputState;
}

const cardStyle: React.CSSProperties = {
  background: "#111",
  borderColor: "#222",
  minHeight: 110,
};

const labelStyle: React.CSSProperties = {
  color: "#666",
  fontSize: 12,
  textTransform: "uppercase" as const,
  letterSpacing: "0.05em",
  marginBottom: 4,
};

const valueStyle: React.CSSProperties = {
  color: "#fff",
  fontSize: 24,
  fontWeight: 600,
  lineHeight: 1.2,
};

const subStyle: React.CSSProperties = {
  color: "#666",
  fontSize: 12,
  marginTop: 4,
};

export default function StatsCards({ quota, curve, throughput }: Props) {
  return (
    <Row gutter={[12, 12]}>
      <Col xs={12} lg={6}>
        <Card style={cardStyle} styles={{ body: { padding: 16 } }}>
          <div style={labelStyle}>Used</div>
          <div style={valueStyle}>{formatBytes(quota.used)}</div>
          <div style={subStyle}>
            Down: {formatBytes(quota.used_download)} / Up:{" "}
            {formatBytes(quota.used_upload)}
          </div>
        </Card>
      </Col>
      <Col xs={12} lg={6}>
        <Card style={cardStyle} styles={{ body: { padding: 16 } }}>
          <div style={labelStyle}>Remaining</div>
          <div style={valueStyle}>{formatBytes(quota.remaining)}</div>
          <div style={subStyle}>{quota.billing_month}</div>
        </Card>
      </Col>
      <Col xs={12} lg={6}>
        <Card style={cardStyle} styles={{ body: { padding: 16 } }}>
          <div style={labelStyle}>Sustained Rate</div>
          <div style={valueStyle}>{formatRateKbit(curve.rate_kbit)}</div>
          <div style={subStyle}>
            shape={curve.shape.toFixed(2)} ratio=
            {curve.down_up_ratio.toFixed(2)}
          </div>
        </Card>
      </Col>
      <Col xs={12} lg={6}>
        <Card style={cardStyle} styles={{ body: { padding: 16 } }}>
          <div style={labelStyle}>Throughput</div>
          <div style={valueStyle}>
            {formatRate(throughput.current_down_bps)}
          </div>
          <div style={subStyle}>
            Up: {formatRate(throughput.current_up_bps)}
          </div>
        </Card>
      </Col>
    </Row>
  );
}
