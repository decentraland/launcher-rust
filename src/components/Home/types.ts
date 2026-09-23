// Mirror of core/src/types.rs. Keep both in sync.

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

export type LaunchStatus =
  | { event: "state"; data: { step: Step } }
  | { event: "error"; data: { message: string } };

export type IssueType = { label: string; optionId: string };

// Each variant is a screen of the crash-report dialog and carries exactly what it renders.
export type ReportStep =
  | { event: "infoSomethingWrongStep"; data: { doNotShowAgain: boolean } }
  | {
      event: "bugReportFormStep";
      data: {
        issueType: string;
        issueTypes: IssueType[];
        description: string;
        shareLogs: boolean;
        submitting: boolean;
        error: string | null;
      };
    }
  | { event: "bugReportSubmittedStep"; data: {} };

export type Status =
  | { event: "launch"; data: LaunchStatus }
  | { event: "report"; data: ReportStep };

// Argument of the `crash_report_set_field` command (core/src/report_flow.rs `CrashReportField`).
export type CrashReportField =
  | { event: "issueType"; data: { optionId: string } }
  | { event: "description"; data: { text: string } }
  | { event: "shareLogs"; data: { value: boolean } }
  | { event: "dontShowAgain"; data: { value: boolean } };

// Argument of the `crash_report_navigate` command (core/src/report_flow.rs `Screen`).
export type ReportScreen = "prompt" | "form";
