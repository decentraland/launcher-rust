import React from "react";
import {
  Box,
  Checkbox,
  CircularProgress,
  FormControlLabel,
  MenuItem,
} from "decentraland-ui2";
import { CrashReportField, ReportScreen, ReportStep } from "../Home/types";
import {
  Body,
  ButtonRow,
  CloseButton,
  DescriptionField,
  ErrorText,
  Field,
  FieldHint,
  FieldLabel,
  Header,
  IssueSelect,
  Panel,
  PrimaryButton,
  SecondaryButton,
  Title,
} from "./Report.styles";
import {
  BugIcon,
  CheckboxBox,
  CloseIcon,
  SuccessIcon,
  WarningIcon,
} from "./Report.icons";

// Every interaction is a Rust command; the snapshot in `step` is the only source of truth.
export type SendCommand = (
  command: string,
  args?: Record<string, unknown>,
) => Promise<void>;

interface ReportViewProps {
  step: ReportStep;
  send: SendCommand;
}

const setField = (send: SendCommand, field: CrashReportField) =>
  send("crash_report_set_field", { field });

const navigate = (send: SendCommand, screen: ReportScreen) =>
  send("crash_report_navigate", { screen });

const checkboxSx = { padding: "6px" };

const labelSx = {
  color: "#FFFFFF",
  "& .MuiFormControlLabel-label": {
    fontFamily: "Inter, sans-serif",
    fontSize: 15,
    color: "#FFFFFF",
  },
};

// Same trick as the EXIT button in Home.tsx: the theme styles contained buttons through
// `:not(:hover)` selectors no class specificity beats, so the secondary look is inline.
const SECONDARY_STYLE = { background: "#2B0B45" };

const CHECKBOX_OFF = <CheckboxBox checked={false} />;
const CHECKBOX_ON = <CheckboxBox checked />;

const REQUIRED_MARK = <span>*</span>;

export const ReportView: React.FC<ReportViewProps> = ({ step, send }) => {
  switch (step.event) {
    case "infoSomethingWrongStep":
      return renderPrompt(step.data.doNotShowAgain, send);
    case "bugReportFormStep":
      return renderForm(step.data, send);
    case "bugReportSubmittedStep":
      return renderSubmitted(send);
  }
};

const renderPrompt = (doNotShowAgain: boolean, send: SendCommand) => (
  <Panel>
    <CloseButton aria-label="close" onClick={() => send("crash_report_close")}>
      <CloseIcon />
    </CloseButton>
    <Header>
      <WarningIcon />
      <Title>Something went wrong</Title>
    </Header>
    <Body>
      We noticed an issue while you were exploring, and Decentraland had to
      close. Would you like to send us a report to help us improve the
      experience?
    </Body>
    <FormControlLabel
      sx={{ mt: 2, ...labelSx }}
      control={
        <Checkbox
          sx={checkboxSx}
          icon={CHECKBOX_OFF}
          checkedIcon={CHECKBOX_ON}
          checked={doNotShowAgain}
          onChange={(_, value) =>
            setField(send, { event: "dontShowAgain", data: { value } })
          }
        />
      }
      label="Don't show this again"
    />
    <ButtonRow>
      <SecondaryButton
        variant="contained"
        style={SECONDARY_STYLE}
        onClick={() => send("relaunch")}
      >
        RELAUNCH
      </SecondaryButton>
      <PrimaryButton variant="contained" onClick={() => navigate(send, "form")}>
        REPORT BUG
      </PrimaryButton>
    </ButtonRow>
  </Panel>
);

const renderForm = (
  data: Extract<ReportStep, { event: "bugReportFormStep" }>["data"],
  send: SendCommand,
) => (
  <Panel>
    <Header>
      <BugIcon />
      <Title>Bug Report</Title>
    </Header>

    <Field>
      <FieldLabel>Issue type{REQUIRED_MARK}</FieldLabel>
      <FieldHint>Select your issue category</FieldHint>
      <IssueSelect
        value={data.issueType}
        disabled={data.submitting}
        onChange={(e) =>
          setField(send, {
            event: "issueType",
            data: { optionId: String(e.target.value) },
          })
        }
      >
        {data.issueTypes.map((t) => (
          <MenuItem key={t.optionId} value={t.optionId}>
            {t.label}
          </MenuItem>
        ))}
      </IssueSelect>
    </Field>

    <Field>
      <FieldLabel>Description{REQUIRED_MARK}</FieldLabel>
      <FieldHint>Briefly describe what happened.</FieldHint>
      <DescriptionField
        multiline
        minRows={5}
        maxRows={5}
        placeholder="Write here"
        value={data.description}
        disabled={data.submitting}
        onChange={(e) =>
          setField(send, {
            event: "description",
            data: { text: e.target.value },
          })
        }
      />
    </Field>

    <FormControlLabel
      sx={labelSx}
      control={
        <Checkbox
          sx={checkboxSx}
          icon={CHECKBOX_OFF}
          checkedIcon={CHECKBOX_ON}
          checked={data.shareLogs}
          disabled={data.submitting}
          onChange={(_, value) =>
            setField(send, { event: "shareLogs", data: { value } })
          }
        />
      }
      label="Share diagnostic logs to help us resolve this issue."
    />

    {data.error ? <ErrorText>{data.error}</ErrorText> : null}

    <ButtonRow>
      <SecondaryButton
        variant="contained"
        style={SECONDARY_STYLE}
        disabled={data.submitting}
        onClick={() => navigate(send, "prompt")}
      >
        CANCEL
      </SecondaryButton>
      <PrimaryButton
        variant="contained"
        disabled={data.submitting}
        onClick={() => send("crash_report_submit")}
      >
        {data.submitting ? (
          <CircularProgress size={20} sx={{ color: "#fff" }} />
        ) : (
          "SUBMIT"
        )}
      </PrimaryButton>
    </ButtonRow>
  </Panel>
);

const renderSubmitted = (send: SendCommand) => (
  <Panel>
    <CloseButton aria-label="close" onClick={() => send("crash_report_close")}>
      <CloseIcon />
    </CloseButton>
    <Header>
      <SuccessIcon />
      <Title>Bug Report Submitted!</Title>
    </Header>
    <Body>
      Thanks for helping us improve Decentraland! We've received your report and
      our team will investigate the issue and work on a fix.
    </Body>
    <Box sx={{ mt: 3, width: 200 }}>
      <PrimaryButton
        fullWidth
        variant="contained"
        onClick={() => send("relaunch")}
      >
        RELAUNCH
      </PrimaryButton>
    </Box>
  </Panel>
);
