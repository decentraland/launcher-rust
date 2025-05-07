export enum BuildType {
  New = "new",
  Update = "update",
}

export type Step =
  | { event: "fetching"; data: {} }
  | { event: "downloading"; data: { progress: number; buildType: BuildType } }
  | { event: "installing"; data: { buildType: BuildType } }
  | { event: "launching"; data: {} };

export type Status =
  | { event: "state"; data: { step: Step } }
  | { event: "error"; data: { message: string; canRetry: boolean } };
