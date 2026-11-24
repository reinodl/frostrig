// Talks to the Rust side of the app, which forwards to the frostrigd service.
import { invoke } from '@tauri-apps/api/core';

export type ModeId = 'silent' | 'balanced' | 'performance' | 'extreme';
export type FanPreset = 'auto' | 'quiet' | 'cool' | 'custom';

export interface FanCurve {
  temps: number[];
  speeds: number[];
}

export interface Gpu {
  /** PCI address; pass it back to the GPU setters. */
  id: string;
  vendor: 'nvidia' | 'amd' | 'intel';
  driver: string;
  name: string;
  integrated: boolean;
  awake: boolean;
  tempC: number | null;
  powerW: number | null;
  powerLimitW: number | null;
  clockMhz: number | null;
  /** null when this GPU's clock can't be capped. limitMhz null = no cap. */
  clock: { minMhz: number; maxMhz: number; stepMhz: number; limitMhz: number | null } | null;
  /** null unless the driver accepted a test write. limitW null = default. */
  power: { minW: number; maxW: number; defaultW: number; limitW: number | null } | null;
  /** AMD only. */
  level: { options: string[]; current: string } | null;
}

export interface FanTest {
  available: boolean;
  running: boolean;
  phase: 'cpu' | 'settle' | 'gpu' | null;
  ok: boolean | null;
  message: string | null;
}

export interface Status {
  version: string;
  device: {
    model: string | null;
    vendor: string | null;
    formFactor: 'laptop' | 'desktop' | 'unknown';
    board: string | null;
    ecFirmware: string | null;
    msiEc: boolean;
  };
  supported: {
    modes: ModeId[];
    modeBackend: 'msi-ec' | 'power-profiles-daemon' | 'platform-profile' | null;
    maxFans: boolean;
    chargeLimit: boolean;
    turbo: boolean;
    cpuPower: boolean;
    fanCurves: boolean;
    fanCurveReason: string | null;
    fanTest: FanTest;
  };
  state: {
    mode: ModeId | null;
    maxFans: boolean | null;
    turbo: boolean | null;
    pl1W: number | null;
    pl2W: number | null;
    powerCustom: boolean;
    fanPreset: FanPreset;
    fanMode: string | null;
  };
  powerLimits: { pl1Min: number; pl1Max: number; pl2Min: number; pl2Max: number } | null;
  /** gpu is null on single-curve boards: the CPU curve drives every fan. */
  fanCurve: { cpu: FanCurve; gpu: FanCurve | null } | null;
  sensors: {
    cpuTemp: number | null;
    gpuTemp: number | null;
    /** Average of the cores. */
    cpuClockMhz: number | null;
    /** Package power from RAPL; null until the service has two samples. */
    cpuPowerW: number | null;
    cpuFan: number | null;
    gpuFan: number | null;
    fans: { label: string; rpm: number }[];
  };
  /** MSI extras; each null when this laptop doesn't have it. */
  extras: {
    webcam: boolean | null;
    webcamBlock: boolean | null;
    fnKey: 'left' | 'right' | null;
    superBattery: boolean | null;
    kbdBacklight: { level: number | null; max: number } | null;
  };
  gpus: Gpu[];
  /** null when nvidia-powerd isn't installed or there's no NVIDIA GPU. */
  dynamicBoost: { on: boolean } | null;
  battery: {
    present: boolean;
    percent: number | null;
    status: string | null;
    acOnline: boolean | null;
    chargeLimit: number | null;
  };
}

export async function getStatus(): Promise<Status> {
  return JSON.parse(await invoke<string>('status')) as Status;
}

export const api = {
  setMode: (mode: ModeId) => invoke<void>('set_mode', { mode }),
  setMaxFans: (on: boolean) => invoke<void>('set_max_fans', { on }),
  setChargeLimit: (percent: number) => invoke<void>('set_charge_limit', { percent }),
  setTurbo: (on: boolean) => invoke<void>('set_turbo', { on }),
  setCpuPower: (sustainedW: number, burstW: number) => invoke<void>('set_cpu_power', { sustainedW, burstW }),
  setFanPreset: (preset: FanPreset) => invoke<void>('set_fan_preset', { preset }),
  setCustomFanCurve: (cpu: FanCurve, gpu: FanCurve) =>
    invoke<void>('set_custom_fan_curve', {
      cpuTemps: cpu.temps, cpuSpeeds: cpu.speeds, gpuTemps: gpu.temps, gpuSpeeds: gpu.speeds
    }),
  resetAdvanced: () => invoke<void>('reset_advanced'),
  startFanTest: () => invoke<void>('start_fan_test'),
  cancelFanTest: () => invoke<void>('cancel_fan_test'),
  report: () => invoke<string>('report'),
  setWebcam: (on: boolean) => invoke<void>('set_webcam', { on }),
  setWebcamBlock: (on: boolean) => invoke<void>('set_webcam_block', { on }),
  setFnKey: (side: 'left' | 'right') => invoke<void>('set_fn_key', { side }),
  setSuperBattery: (on: boolean) => invoke<void>('set_super_battery', { on }),
  setKeyboardBacklight: (level: number) => invoke<void>('set_keyboard_backlight', { level }),
  /** 0 removes the limit. */
  setGpuClockLimit: (gpu: string, mhz: number) => invoke<void>('set_gpu_clock_limit', { gpu, mhz }),
  /** 0 goes back to the driver default. */
  setGpuPowerLimit: (gpu: string, watts: number) => invoke<void>('set_gpu_power_limit', { gpu, watts }),
  setGpuLevel: (gpu: string, level: string) => invoke<void>('set_gpu_level', { gpu, level }),
  setDynamicBoost: (on: boolean) => invoke<void>('set_dynamic_boost', { on }),
  setCloseToTray: (on: boolean) => invoke<void>('set_close_to_tray', { on }),
  getAutostart: () => invoke<boolean>('get_autostart'),
  setAutostart: (on: boolean) => invoke<void>('set_autostart', { on })
};

export function errorText(e: unknown): string {
  if (typeof e === 'string') return e;
  if (e instanceof Error) return e.message;
  return 'Something went wrong.';
}
