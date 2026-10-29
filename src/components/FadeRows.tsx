// The Region inspector's fade rows, as in Logic: Fade In (or Speed Up) and its Curve, Fade Out
// (or Slow Down), its Type (Out or one of the crossfades) and its Curve.
import { act, type Region } from "../api";
import { crossfadesByRegion, curveToUi, FADE_OUT_TYPES, isCrossfadeType, shapeOf, uiToCurve, type FadeInType, type FadeOutType, type FadeShape } from "../fades";
import { Icon } from "../icons";
import { useStore } from "../store";
import { NumberField } from "./Form";
import { openMenuAt } from "./Menu";

/** A label or value that opens a menu of choices, drawn as text with a small chevron. */
function InlinePopup<T extends string>({ value, options, onChange }: { value: T; options: [T, string][]; onChange: (v: T) => void }) {
  return (
    <button
      type="button"
      className="insp-popup"
      onMouseDown={(e) => {
        e.preventDefault();
        e.stopPropagation();
        openMenuAt(
          e.currentTarget,
          options.map(([v, label]) => ({ label, checked: v === value, onClick: () => onChange(v) })),
        );
      }}
    >
      {options.find((o) => o[0] === value)?.[1] ?? ""}
      <Icon.chevron />
    </button>
  );
}

function Row({ label, children, title, replaced }: { label: React.ReactNode; children: React.ReactNode; title?: string; replaced?: boolean }) {
  return (
    <div className={"insp-row" + (replaced ? " replaced" : "")} title={title}>
      <span className="insp-label">{label}</span>
      <span className="insp-value">{children}</span>
    </div>
  );
}

export function FadeRows({ region, sampleRate }: { region: Region; sampleRate: number }) {
  const crossfades = useStore((s) => s.snap?.crossfades);
  if (region.content.type !== "audio") return null;
  const c = region.content;
  const shape = shapeOf(c);
  const xf = crossfadesByRegion(crossfades).get(region.id);
  const ids = [region.id];
  const ms = (frames: number) => Math.round((frames / sampleRate) * 1000);
  const frames = (v: number) => Math.round((v / 1000) * sampleRate);
  const setShape = (patch: { inCurve?: number; outCurve?: number; inType?: FadeInType; outType?: FadeOutType }) => act("set_region_fade_shape", { ids, ...patch });
  const slow = shape.out_type === "slow_down";
  // Crossfaded into from the previous region: the fade-in settings aren't used meanwhile.
  const replaced = xf?.in ? "The crossfade from the previous region replaces the fade-in" : undefined;
  const curve = (value: number, set: (c: number) => void) => <NumberField value={curveToUi(value)} step={10} resolution={1} min={-100} max={100} width={70} onChange={(v) => set(uiToCurve(v))} />;
  const outMode: FadeOutType = slow ? "slow_down" : "out";
  return (
    <>
      <Row
        label={<InlinePopup<FadeInType> value={shape.in_type} options={[["fade", "Fade In"], ["speed_up", "Speed Up"]]} onChange={(inType) => setShape({ inType })} />}
        title={replaced}
        replaced={!!replaced}
      >
        <NumberField value={ms(c.fade_in)} step={10} resolution={1} min={0} width={70} onChange={(v) => act("set_region_fades", { ids, fadeIn: frames(v), fadeOut: null })} />
        <span className="dim"> ms</span>
      </Row>
      <Row label="Curve" title={replaced} replaced={!!replaced}>
        {curve(shape.in_curve, (inCurve) => setShape({ inCurve }))}
      </Row>
      <Row
        label={
          <InlinePopup<FadeOutType>
            value={outMode}
            options={[["out", "Fade Out"], ["slow_down", "Slow Down"]]}
            // Back from Slow Down to a plain fade-out; crossfade types are chosen under Type.
            onChange={(outType) => outType !== outMode && setShape({ outType })}
          />
        }
      >
        <NumberField value={ms(c.fade_out)} step={10} resolution={1} min={0} width={70} onChange={(v) => act("set_region_fades", { ids, fadeIn: null, fadeOut: frames(v) })} />
        <span className="dim"> ms</span>
      </Row>
      <Row label="Type" title={crossfadeNote(shape, !!xf?.out)}>
        <InlinePopup<FadeOutType> value={slow ? "out" : shape.out_type} options={FADE_OUT_TYPES} onChange={(outType) => setShape({ outType })} />
      </Row>
      <Row label="Curve">{curve(shape.out_curve, (outCurve) => setShape({ outCurve }))}</Row>
    </>
  );
}

/** The Type row's tooltip: whether a crossfade type is crossfading. */
function crossfadeNote(shape: FadeShape, active: boolean): string | undefined {
  if (!isCrossfadeType(shape.out_type)) return undefined;
  return active ? "Crossfades with the following region" : "No region follows directly, so this fades out like Out";
}
