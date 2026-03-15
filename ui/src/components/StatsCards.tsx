import { Row, Col } from "antd";
import type { CurveState } from "../types";
import { formatRateKbit } from "../utils";

interface Props {
  curve: CurveState;
}

const cardStyle: React.CSSProperties = {
  background: "#111",
  border: "1px solid #222",
  borderRadius: 8,
  padding: 14,
};

const labelStyle: React.CSSProperties = {
  color: "#666",
  fontSize: 11,
  textTransform: "uppercase" as const,
  letterSpacing: "0.05em",
  marginBottom: 2,
};

const valueStyle: React.CSSProperties = {
  color: "#fff",
  fontSize: 22,
  fontWeight: 600,
  lineHeight: 1.2,
};

const subStyle: React.CSSProperties = {
  color: "#555",
  fontSize: 11,
  marginTop: 2,
};

export default function StatsCards({ curve }: Props) {
  return (
    <Row gutter={[10, 10]}>
      <Col xs={24}>
        <div style={cardStyle}>
          <div style={labelStyle}>Sustained Rate</div>
          <div style={valueStyle}>{formatRateKbit(curve.rate_kbit)}</div>
          <div style={subStyle}>
            shape={curve.shape.toFixed(2)} ratio=
            {curve.down_up_ratio.toFixed(2)}
          </div>
        </div>
      </Col>
    </Row>
  );
}
