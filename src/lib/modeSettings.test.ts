import { describe, expect, it } from "vitest";
import {
  BITRATE_VALUE,
  DEFAULT_SCROLL,
  MIRROR_MODES,
  allowsControl,
  applyQuality,
  defaultModeDefaults,
  deriveQuality,
  fileSyncMigrationPatch,
  keepsAspectRatio,
  migrateLegacy,
  nearestCameraBitrate,
  normalizeModeDefaults,
  playsAudio,
  resolveLaunchSettings,
  segmentForBitrate,
  segmentForFps,
  segmentForResolution,
  toCameraSettings,
  toScrollConfig,
  usesOnScreenKeyboard,
  type ModeDefaultsV2,
} from "./modeSettings";
import { DEFAULT_SETTINGS, PRESET_BUNDLES, type CoreSettings } from "./types";

const CORE_KEYS = Object.keys(DEFAULT_SETTINGS).sort();
const DISPLAY = ["mirror", "flex", "hifi"] as const;

describe("defaults and resolver", () => {
  it("fresh-install defaults follow SPEC-28 section 5.3", () => {
    const d = defaultModeDefaults();
    expect(d.version).toBe(2);
    for (const m of ["mirror", "flex"] as const) {
      expect(d[m]).toMatchObject(PRESET_BUNDLES.medium);
    }
    expect(d.hifi).toMatchObject({
      maxSize: 0,
      videoBitRate: 20_000_000,
      maxFps: 60,
      videoCodec: "h265",
    });
    expect(d.camera).toEqual({ playAudio: true, bitrate: "balanced", embedded: true });
    for (const m of DISPLAY) {
      expect(d[m].embedded).toBe(true);
      expect(d[m].noAudio).toBe(false);
      expect("flexDisplay" in d[m]).toBe(false);
    }
  });

  it("resolveLaunchSettings returns all 17 CoreSettings keys", () => {
    const d = defaultModeDefaults();
    for (const m of DISPLAY) {
      expect(Object.keys(resolveLaunchSettings(m, d)).sort()).toEqual(CORE_KEYS);
    }
    expect(CORE_KEYS).toHaveLength(17);
  });

  it("flexDisplay is true only for flex", () => {
    const d = defaultModeDefaults();
    expect(resolveLaunchSettings("flex", d).flexDisplay).toBe(true);
    expect(resolveLaunchSettings("mirror", d).flexDisplay).toBe(false);
    expect(resolveLaunchSettings("hifi", d).flexDisplay).toBe(false);
  });

  it("switching or resolving modes never mutates another mode", () => {
    const d = defaultModeDefaults();
    const before = structuredClone(d);
    for (const m of ["flex", "mirror", "hifi", "mirror", "flex"] as const) {
      resolveLaunchSettings(m, d);
    }
    expect(d).toEqual(before);
  });

  it("editing mode A leaves B, C, D deep-equal", () => {
    const d = defaultModeDefaults();
    const before = structuredClone(d);
    const edited: ModeDefaultsV2 = { ...d, mirror: { ...d.mirror, noAudio: true, maxFps: 30 } };
    expect(edited.flex).toEqual(before.flex);
    expect(edited.hifi).toEqual(before.hifi);
    expect(edited.camera).toEqual(before.camera);
    expect(d).toEqual(before);
  });

  it("toCameraSettings maps defaults and lets per-launch choices win", () => {
    const c = toCameraSettings(
      { playAudio: false, bitrate: "high", embedded: true },
      { facing: "back" },
    );
    expect(c).toEqual({ facing: "back", noAudio: true, embedded: true, videoBitRate: 16_000_000 });
    expect(
      toCameraSettings(
        { playAudio: true, bitrate: "low", embedded: false },
        { facing: "front", videoBitRate: 4_000_000 },
      ).videoBitRate,
    ).toBe(4_000_000);
  });

  it("normalizeModeDefaults fills partial objects and drops flexDisplay", () => {
    const n = normalizeModeDefaults({
      mirror: { maxFps: 30, flexDisplay: true },
      camera: { playAudio: false },
    });
    expect(n.mirror.maxFps).toBe(30);
    expect("flexDisplay" in n.mirror).toBe(false);
    expect(n.flex).toEqual(defaultModeDefaults().flex);
    expect(n.camera).toEqual({ playAudio: false, bitrate: "balanced", embedded: true });
    expect(normalizeModeDefaults(null)).toEqual(defaultModeDefaults());
  });
});

describe("migrateLegacy (SPEC-28 section 5.4)", () => {
  const legacy = (over: Partial<CoreSettings> = {}): Partial<CoreSettings> => ({
    ...DEFAULT_SETTINGS,
    ...over,
  });

  it("(a) default legacy medium gives four modes with equal quality and audio on", () => {
    const { modeDefaults: m, audioNotice } = migrateLegacy(legacy(), "medium");
    expect(audioNotice).toBe(false);
    expect(m.mirror).toMatchObject(PRESET_BUNDLES.medium);
    expect(m.flex).toMatchObject(PRESET_BUNDLES.medium);
    expect(m.hifi.maxSize).toBe(PRESET_BUNDLES.medium.maxSize);
    for (const k of DISPLAY) expect(m[k].noAudio).toBe(false);
    expect(m.camera.playAudio).toBe(true);
  });

  it("(b) Camera residue noAudio gives audio on everywhere, notice set", () => {
    const { modeDefaults: m, audioNotice } = migrateLegacy(legacy({ noAudio: true }), "medium");
    for (const k of DISPLAY) expect(m[k].noAudio).toBe(false);
    expect(m.camera.playAudio).toBe(true);
    expect(audioNotice).toBe(true);
  });

  it("(c) hi-fi signature: mirror/flex = preset bundle, hifi = legacy values", () => {
    const l = legacy({ videoCodec: "av1", videoBitRate: 20_000_000, maxFps: 60, maxSize: 0 });
    const { modeDefaults: m } = migrateLegacy(l, "high");
    expect(m.mirror).toMatchObject(PRESET_BUNDLES.high);
    expect(m.flex).toMatchObject(PRESET_BUNDLES.high);
    expect(m.hifi).toMatchObject({
      videoCodec: "av1",
      videoBitRate: 20_000_000,
      maxFps: 60,
      maxSize: 0,
    });
  });

  it("(d) legacy flexDisplay + unscaled stays on flex only", () => {
    const { modeDefaults: m } = migrateLegacy(
      legacy({ flexDisplay: true, renderFit: "unscaled", noWindowAspectRatioLock: true }),
      "medium",
    );
    expect(m.flex).toMatchObject({ renderFit: "unscaled", noWindowAspectRatioLock: true });
    for (const k of ["mirror", "hifi"] as const) {
      expect(m[k]).toMatchObject({ renderFit: "letterbox", noWindowAspectRatioLock: false });
    }
  });

  it("(d2) legacy flexDisplay off: mirror/hifi keep the pair, flex gets constants", () => {
    const { modeDefaults: m } = migrateLegacy(
      legacy({ flexDisplay: false, renderFit: "letterbox", noWindowAspectRatioLock: true }),
      "medium",
    );
    expect(m.mirror.noWindowAspectRatioLock).toBe(true);
    expect(m.hifi.noWindowAspectRatioLock).toBe(true);
    expect(m.flex).toMatchObject({ renderFit: "unscaled", noWindowAspectRatioLock: true });
  });

  it("(e) stretched is kept in every mode", () => {
    for (const flexDisplay of [true, false]) {
      const { modeDefaults: m } = migrateLegacy(
        legacy({ renderFit: "stretched", flexDisplay }),
        "medium",
      );
      for (const k of DISPLAY) expect(m[k].renderFit).toBe("stretched");
    }
  });

  it("(f) preset custom with hi-fi signature falls back to the medium bundle", () => {
    const l = legacy({ videoCodec: "av1", videoBitRate: 20_000_000, maxFps: 60 });
    const { modeDefaults: m } = migrateLegacy(l, "custom");
    expect(m.mirror).toMatchObject(PRESET_BUNDLES.medium);
    expect(m.flex).toMatchObject(PRESET_BUNDLES.medium);
  });

  it("(g) missing or partial legacy data is filled from defaults", () => {
    expect(migrateLegacy(null).modeDefaults).toEqual(defaultModeDefaults());
    expect(migrateLegacy(undefined, "high").audioNotice).toBe(false);
    const { modeDefaults: m } = migrateLegacy({ maxFps: 30 }, undefined);
    expect(m.mirror.maxFps).toBe(30);
    expect(m.mirror.stayAwake).toBe(DEFAULT_SETTINGS.stayAwake);
    expect(m.hifi.maxFps).toBe(60);
  });

  it("(h) no migrated mode ever has noAudio === true (random legacy objects)", () => {
    let seed = 42;
    const rnd = () => {
      seed = (seed * 1664525 + 1013904223) % 4294967296;
      return seed / 4294967296;
    };
    const pick = <T,>(a: readonly T[]): T => a[Math.floor(rnd() * a.length)];
    for (let i = 0; i < 300; i++) {
      const l: Partial<CoreSettings> = {
        maxSize: pick([0, 800, 1280, 1920]),
        videoBitRate: pick([2e6, 8e6, 20e6, 30e6, 50e6]),
        maxFps: pick([30, 60, 120]),
        videoCodec: pick(["h264", "h265", "av1"] as const),
        noAudio: rnd() < 0.7,
        flexDisplay: rnd() < 0.5,
        renderFit: pick(["letterbox", "unscaled", "stretched"] as const),
        noWindowAspectRatioLock: rnd() < 0.5,
        embedded: rnd() < 0.5,
      };
      const { modeDefaults: m } = migrateLegacy(
        l,
        pick(["low", "medium", "high", "highest", "custom"]),
      );
      for (const k of DISPLAY) expect(m[k].noAudio).toBe(false);
      expect(m.camera.playAudio).toBe(true);
    }
  });

  it("(i) is pure, deterministic and leaves its input untouched", () => {
    const l = legacy({ noAudio: true, flexDisplay: true });
    const copy = structuredClone(l);
    expect(migrateLegacy(l, "high")).toEqual(migrateLegacy(l, "high"));
    expect(l).toEqual(copy);
  });

  it("(j) camera bitrate is the nearest of low / balanced / high", () => {
    expect(nearestCameraBitrate(1_000_000)).toBe("low");
    expect(nearestCameraBitrate(3_000_000)).toBe("low");
    expect(nearestCameraBitrate(6_000_000)).toBe("balanced");
    expect(nearestCameraBitrate(8_000_000)).toBe("balanced");
    expect(nearestCameraBitrate(13_000_000)).toBe("high");
    expect(nearestCameraBitrate(50_000_000)).toBe("high");
    expect(migrateLegacy(legacy({ videoBitRate: 2_000_000 })).modeDefaults.camera.bitrate).toBe(
      "low",
    );
  });

  it("camera.embedded copies legacy embedded, defaulting to false", () => {
    expect(migrateLegacy(legacy({ embedded: true })).modeDefaults.camera.embedded).toBe(true);
    expect(migrateLegacy(legacy({ embedded: false })).modeDefaults.camera.embedded).toBe(false);
    expect(migrateLegacy({ maxFps: 60 }).modeDefaults.camera.embedded).toBe(false);
  });

  it("step 1 keeps everything else the user chose in every mode", () => {
    const { modeDefaults: m } = migrateLegacy(
      legacy({
        stayAwake: false,
        turnScreenOff: true,
        fullscreen: true,
        showTouches: true,
        noControl: true,
        noKeyboardIme: true,
        hideVirtualTaskbar: true,
        flexDisplaySize: "1920x1200/200",
      }),
      "medium",
    );
    for (const k of DISPLAY) {
      expect(m[k]).toMatchObject({
        stayAwake: false,
        turnScreenOff: true,
        fullscreen: true,
        showTouches: true,
        noControl: true,
        noKeyboardIme: true,
        hideVirtualTaskbar: true,
        flexDisplaySize: "1920x1200/200",
      });
      expect("flexDisplay" in m[k]).toBe(false);
    }
  });
});

describe("KVM file sync migration (step 6)", () => {
  it("syncFileTransfer=false with file copy-paste on turns file copy-paste off", () => {
    expect(
      fileSyncMigrationPatch({ syncFileTransfer: false, continuityFileClipboard: true }),
    ).toEqual({ continuityFileClipboard: false });
  });
  it("does nothing otherwise", () => {
    expect(
      fileSyncMigrationPatch({ syncFileTransfer: true, continuityFileClipboard: true }),
    ).toBeNull();
    expect(
      fileSyncMigrationPatch({ syncFileTransfer: false, continuityFileClipboard: false }),
    ).toBeNull();
    expect(fileSyncMigrationPatch({})).toBeNull();
  });
});

describe("quality derivation and helpers", () => {
  it("maps the four bundles to Smooth / Balanced / Sharp / Maximum", () => {
    expect(deriveQuality(PRESET_BUNDLES.low)).toBe("smooth");
    expect(deriveQuality(PRESET_BUNDLES.medium)).toBe("balanced");
    expect(deriveQuality(PRESET_BUNDLES.high)).toBe("sharp");
    expect(deriveQuality(PRESET_BUNDLES.highest)).toBe("maximum");
  });
  it("any deviation is Custom", () => {
    expect(deriveQuality({ ...PRESET_BUNDLES.medium, maxFps: 30 })).toBe("custom");
    expect(deriveQuality({ ...PRESET_BUNDLES.high, videoCodec: "av1" })).toBe("custom");
  });
  it("applyQuality round-trips and returns null for custom", () => {
    for (const c of ["smooth", "balanced", "sharp", "maximum"] as const) {
      expect(deriveQuality(applyQuality(c)!)).toBe(c);
    }
    expect(applyQuality("custom")).toBeNull();
  });
  it("resolution segments including odd values", () => {
    expect(segmentForResolution(0)).toBe("native");
    expect(segmentForResolution(1920)).toBe("1080p");
    expect(segmentForResolution(1280)).toBe("720p");
    expect(segmentForResolution(800)).toBe("custom");
    expect(segmentForResolution(1600)).toBe("custom");
  });
  it("bitrate segments: low <= 4, balanced <= 12, high above", () => {
    expect(segmentForBitrate(2_000_000)).toBe("low");
    expect(segmentForBitrate(4_000_000)).toBe("low");
    expect(segmentForBitrate(4_000_001)).toBe("balanced");
    expect(segmentForBitrate(12_000_000)).toBe("balanced");
    expect(segmentForBitrate(12_000_001)).toBe("high");
    expect(segmentForBitrate(BITRATE_VALUE.high)).toBe("high");
  });
  it("frame rate segments", () => {
    expect(segmentForFps(30)).toBe(30);
    expect(segmentForFps(60)).toBe(60);
    expect(segmentForFps(120)).toBe("custom");
  });
  it("positive helpers invert the stored flags and round-trip", () => {
    for (const v of [true, false]) {
      expect(playsAudio({ noAudio: v })).toBe(!v);
      expect(allowsControl({ noControl: v })).toBe(!v);
      expect(usesOnScreenKeyboard({ noKeyboardIme: v })).toBe(!v);
      expect(keepsAspectRatio({ noWindowAspectRatioLock: v })).toBe(!v);
    }
  });
  it("covers all four modes", () => {
    expect(MIRROR_MODES).toEqual(["mirror", "flex", "camera", "hifi"]);
  });
});

describe("scroll settings (SPEC-30)", () => {
  it("provides expected per-mode defaults", () => {
    const d = defaultModeDefaults();
    expect(d.mirror.scroll).toEqual({
      style: "smooth",
      speed: "auto",
      touchpadSpeed: 1,
      smooth: true,
      inertia: "off",
    });
    expect(d.flex.scroll).toEqual({
      style: "smooth",
      speed: "auto",
      touchpadSpeed: 1,
      smooth: true,
      inertia: "off",
    });
    expect(d.hifi.scroll).toEqual({
      style: "smooth",
      speed: "auto",
      touchpadSpeed: 1,
      smooth: false,
      inertia: "off",
    });
    expect(DEFAULT_SCROLL.camera).toEqual({
      style: "smooth",
      speed: "auto",
      touchpadSpeed: 1,
      smooth: false,
      inertia: "off",
    });
  });

  it("fills missing scroll settings and missing smooth/touchpadSpeed fields on older modeDefaults without version bump", () => {
    const old = {
      version: 2,
      mirror: { maxFps: 30 },
      flex: { renderFit: "unscaled", scroll: { style: "smooth", speed: "auto" } },
      hifi: { maxSize: 0, scroll: { style: "smooth", speed: 2 } },
    };
    const n = normalizeModeDefaults(old);
    expect(n.version).toBe(2);
    expect(n.mirror.maxFps).toBe(30);
    expect(n.mirror.scroll).toEqual(DEFAULT_SCROLL.mirror);
    expect(n.flex.scroll).toEqual({
      style: "smooth",
      speed: "auto",
      touchpadSpeed: 1,
      smooth: true,
      inertia: "off",
    });
    expect(n.hifi.scroll).toEqual({
      style: "smooth",
      speed: 2,
      touchpadSpeed: 1,
      smooth: false,
      inertia: "off",
    });
  });

  it("clamps manual speed and touchpadSpeed and normalizes invalid scroll values", () => {
    const n = normalizeModeDefaults({
      mirror: {
        scroll: { style: "bogus", speed: 99, touchpadSpeed: 9, smooth: "nope", inertia: "mega" },
      },
      flex: { scroll: { style: "paged", speed: 0.25, touchpadSpeed: 0.1 } },
    });
    expect(n.mirror.scroll).toEqual({
      style: "smooth",
      speed: 5,
      touchpadSpeed: 3,
      smooth: true,
      inertia: "off",
    });
    expect(n.flex.scroll).toEqual({
      style: "paged",
      speed: 1,
      touchpadSpeed: 0.5,
      smooth: true,
      inertia: "off",
    });
  });

  it("toScrollConfig maps auto, manual, and touchpadSpeed for IPC", () => {
    expect(toScrollConfig(DEFAULT_SCROLL.mirror)).toEqual({
      style: "smooth",
      speedMode: "auto",
      manual: 1,
      touchpadSpeed: 1,
      smooth: true,
      inertia: "off",
    });
    expect(
      toScrollConfig({
        style: "paged",
        speed: 2.5,
        touchpadSpeed: 1.75,
        smooth: false,
        inertia: "off",
      }),
    ).toEqual({
      style: "paged",
      speedMode: "manual",
      manual: 2.5,
      touchpadSpeed: 1.75,
      smooth: false,
      inertia: "off",
    });
  });
});
