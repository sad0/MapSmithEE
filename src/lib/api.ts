import { invoke } from '@tauri-apps/api/core';
import type { Are, AreaDoc, GameInfo, Maps, Settings, WedEdit } from './types';

export const api = {
  settingsGet: () => invoke<Settings>('settings_get'),
  settingsSet: (settings: Settings) => invoke<void>('settings_set', { settings }),
  gameOpen: (root: string) => invoke<GameInfo>('game_open', { root }),
  areaOpen: (name: string) => invoke<AreaDoc>('area_open', { name }),
  areaSave: (req: { name: string; are: Are; wed_day: WedEdit; wed_night: WedEdit | null; maps: Maps }) =>
    invoke<string[]>('area_save', { req }),
  areaInstall: (name: string, target: string) => invoke<string[]>('area_install', { name, target }),
  tlk: (strref: number) => invoke<string>('tlk_string', { strref }),
};
