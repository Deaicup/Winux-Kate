import { create } from "zustand";

export interface IdeInstance {
  hwnd: number;
  folder: string | null;
  title: string;
}

export type ImView = "split" | "wecom";

export interface CustomPage {
  id: number;
  name: string;
  exe: string;
  args: string;
}

export interface AppDetection {
  vscode: boolean;
  wechat: boolean;
  qq: boolean;
  wecom: boolean;
}

interface AppStore {
  booted: boolean;
  currentPage: number;
  ideList: IdeInstance[];
  ideActive: number;
  imView: ImView;
  customPages: CustomPage[];
  detection: AppDetection | null;
  setBooted: (b: boolean) => void;
  setPage: (p: number) => void;
  setIde: (list: IdeInstance[], active: number) => void;
  setImView: (v: ImView) => void;
  setCustomPages: (p: CustomPage[]) => void;
  setDetection: (d: AppDetection) => void;
}

export const useStore = create<AppStore>((set) => ({
  booted: false,
  currentPage: 1,
  ideList: [],
  ideActive: 0,
  imView: "split",
  customPages: [],
  detection: null,
  setBooted: (b) => set({ booted: b }),
  setPage: (p) => set({ currentPage: p }),
  setIde: (list, active) => set({ ideList: list, ideActive: active }),
  setImView: (v) => set({ imView: v }),
  setCustomPages: (p) => set({ customPages: p }),
  setDetection: (d) => set({ detection: d }),
}));
