import React, { memo, useEffect, useState } from "react";
import { Box, Typography } from "decentraland-ui2";
import { Status, BuildType } from "./types";
import {
  Landscape,
  LoadingBar,
  Logo,
  ErrorIcon,
  ErrorDialogButton,
} from "./Home.styles";

import LANDSCAPE_IMG from "../../assets/background.jpg";
import LOGO_SVG from "../../assets/logo.svg";
import ERROR_SVG from "../../assets/error.svg";

import { invoke, Channel } from "@tauri-apps/api/core";
import { LogicalSize, getCurrentWindow } from "@tauri-apps/api/window";
import { exit } from "@tauri-apps/plugin-process";

type WindowSize = {
  width: number;
  height: number;
};

function asLogicalSize(size: WindowSize) {
  return new LogicalSize(size.width, size.height);
}

const stateWindowSize = {
  width: 600,
  height: 156,
};

const errorWindowSize = {
  width: 600,
  height: 358,
};

const resizeWindow = async (size: WindowSize) => {
  const logicalSize = asLogicalSize(size);
  await getCurrentWindow().setSize(logicalSize).catch(console.error);
};

const useChannelUpdates = (channel: Channel<Status>) => {
  const [currentStatus, setCurrentStatus] = useState<Status | null>(null);

  useEffect(() => {
    const handleUpdate = (message: Status) => {
      setCurrentStatus(message);
    };
    channel.onmessage = handleUpdate;
  }, [channel]);

  return currentStatus;
};

export const Home: React.FC = memo(() => {
  const [channel] = useState(new Channel<Status>());
  const currentStatus = useChannelUpdates(channel);

  const launchFlow = async () => {
    await invoke("launch", { channel }).catch(console.error);
  };

  useEffect(() => {
    launchFlow();
  }, []);

  const renderStatusMessage = () => {
    if (!currentStatus) return null;

    switch (currentStatus.event) {
      case "state":
        switch (currentStatus.data.step.event) {
          case "launcherUpdate": {
            const data = currentStatus.data.step.data;
            switch (data.event) {
              case "checkingForUpdate":
                return renderStep("Checking for update...");
              case "downloading": {
                const progress = data.data.progress ?? undefined;
                return renderStep("Downloading update...", progress);
              }
              case "downloadFinished":
                return renderStep("Update downloaded...");
              case "installingUpdate":
                return renderStep("Installing update...");
              case "restartingApp":
                return renderStep("Restarting app...");
            }
          }
          case "fetching":
            return renderFetchStep();
          case "downloading": {
            let data = currentStatus.data.step.data;
            let isUpdate = data.buildType === BuildType.Update;
            let progress = data.progress;
            return renderDownloadStep(isUpdate, progress);
          }
          case "installing":
            let data = currentStatus.data.step.data;
            let isUpdate = data.buildType === BuildType.Update;
            return renderInstallStep(isUpdate);
          case "launching":
            return renderLaunchStep();
        }
      case "error":
        return renderError(currentStatus.data.message);
      default:
        return null;
    }
  };

  const renderFetchStep = () => renderStep("Fetching Latest...");

  const renderDownloadStep = (isUpdate: boolean, downloadingProgress: number) =>
    renderStep(
      isUpdate ? "Downloading Update..." : "Downloading Decentraland...",
      downloadingProgress,
    );

  const renderInstallStep = (isUpdate: boolean) =>
    renderStep(
      isUpdate ? "Installing Update..." : "Installation in Progress...",
    );

  const renderLaunchStep = () => renderStep("Launching Decentraland...");

  const renderError = (message: string) => {
    resizeWindow(errorWindowSize);
    return (
      <Box
        display="flex"
        flexDirection="column"
        alignItems="center"
        gap={2}
        sx={{ maxWidth: "400px" }}
      >
        <ErrorIcon src={ERROR_SVG} />
        <Typography
          variant="h5"
          sx={{
            fontFamily: "Inter, sans-serif",
            fontWeight: 700,
          }}
        >
          Error
        </Typography>
        <Typography
          variant="h6"
          sx={{
            fontFamily: "Inter, sans-serif",
            textAlign: "center",
          }}
        >
          {message}
        </Typography>
        <Box display="flex" gap={2} sx={{ pt: 2 }}>
          <ErrorDialogButton
            variant="contained"
            style={{
              backgroundColor: "rgba(0, 0, 0, 0.4)",
            }}
            onClick={() => exit()}
          >
            EXIT
          </ErrorDialogButton>
          <ErrorDialogButton variant="contained" onClick={launchFlow}>
            RETRY
          </ErrorDialogButton>
        </Box>
      </Box>
    );
  };

  const renderStep = (
    message: string,
    downloadingProgress: number | undefined = undefined,
  ) => {
    resizeWindow(stateWindowSize);
    return (
      <>
        <Logo src={LOGO_SVG} />
        <Box
          display="flex"
          flexDirection="column"
          justifyContent="space-between"
          height="61px"
        >
          <Typography
            variant="h6"
            align="left"
            sx={{
              fontFamily: "Inter, sans-serif",
              fontWeight: 700,
              fontSize: "20px",
              lineHeight: "160%",
              letterSpacing: "0px",
              verticalAlign: "middle",
            }}
          >
            {message}
          </Typography>
          <Box
            sx={{
              display: "flex",
              alignItems: "center",
              justifyContent: "center",
            }}
          >
            <LoadingBar
              variant={downloadingProgress ? "determinate" : undefined}
              value={downloadingProgress ?? undefined}
              sx={{ mr: 1 }}
            />
            {
              <Typography
                variant="body1"
                width="45px"
                visibility={downloadingProgress ? "visible" : "hidden"}
              >{`${Math.round(downloadingProgress ?? 0)}%`}</Typography>
            }
          </Box>
        </Box>
      </>
    );
  };

  return (
    <Box
      display="flex"
      alignItems={"center"}
      justifyContent={"center"}
      width={"100%"}
      gap={4}
    >
      <Landscape>
        <img src={LANDSCAPE_IMG} />
      </Landscape>
      {renderStatusMessage()}
    </Box>
  );
});
