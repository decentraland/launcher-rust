export enum BuildType {
  New = "new",
  Update = "update",
}

export type LauncherUpdate =
  | { event: "checkingForUpdate"; data: {} }
  | { event: "downloading"; data: { progress: number | null } }
  | { event: "downloadFinished"; data: {} }
  | { event: "installingUpdate"; data: {} }
  | { event: "restartingApp"; data: {} };

export type Step =
  | { event: "launcherUpdate"; data: LauncherUpdate }
  | { event: "fetching"; data: {} }
  | { event: "deeplinkOpening"; data: {} }
  | { event: "downloading"; data: { progress: number; buildType: BuildType } }
  | { event: "installing"; data: { buildType: BuildType } }
  | { event: "launching"; data: {} };

export type Status =
  | { event: "state"; data: { step: Step } }
  | { event: "error"; data: { message: string } };
