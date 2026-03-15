/**
 * Generate curve points with arc-length parameterization.
 * Points are evenly spaced along the curve path, not along the X axis.
 * This puts more points where the curve is steep for smoother rendering.
 */
export function arcLengthCurvePoints(
  shape: number,
  minRate: number,
  maxRate: number,
  padLeft: number,
  padTop: number,
  pw: number,
  ph: number,
  outputPoints: number = 200,
): { x: number; y: number }[] {
  // 1. Fine uniform sampling
  const fine = 2000;
  const raw: { x: number; y: number }[] = [];
  for (let i = 0; i <= fine; i++) {
    const ratio = i / fine; // remaining ratio (1=full, 0=empty)
    const curved = Math.pow(ratio, shape);
    const rate = minRate + (maxRate - minRate) * curved;
    const x = padLeft + (1 - ratio) * pw;
    const y = padTop + ph - (rate / maxRate) * ph;
    raw.push({ x, y });
  }

  // 2. Cumulative arc length
  const arcLen: number[] = [0];
  for (let i = 1; i < raw.length; i++) {
    const dx = raw[i].x - raw[i - 1].x;
    const dy = raw[i].y - raw[i - 1].y;
    arcLen.push(arcLen[i - 1] + Math.sqrt(dx * dx + dy * dy));
  }
  const totalLen = arcLen[arcLen.length - 1];
  if (totalLen === 0) return raw;

  // 3. Resample at uniform arc-length intervals
  const result: { x: number; y: number }[] = [raw[0]];
  let j = 1;
  for (let i = 1; i < outputPoints; i++) {
    const targetLen = (i / (outputPoints - 1)) * totalLen;
    while (j < arcLen.length - 1 && arcLen[j] < targetLen) j++;
    // Lerp between j-1 and j
    const segLen = arcLen[j] - arcLen[j - 1];
    const t = segLen > 0 ? (targetLen - arcLen[j - 1]) / segLen : 0;
    result.push({
      x: raw[j - 1].x + (raw[j].x - raw[j - 1].x) * t,
      y: raw[j - 1].y + (raw[j].y - raw[j - 1].y) * t,
    });
  }

  return result;
}
