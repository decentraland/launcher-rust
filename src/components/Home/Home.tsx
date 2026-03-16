import React, { memo, useEffect, useState } from "react";
import { Box, Typography, IconButton } from "decentraland-ui2";
import { Status, BuildType, Progress } from "./types";
import {
  LoadingBar,
  Logo,
  ErrorIcon,
  ErrorDialogButton,
} from "./Home.styles";
import { versionLabel } from "./VersionLabel";

import LANDSCAPE_IMG from "../../assets/background.jpg";
import LOGO_SVG from "../../assets/logo.svg";
import ERROR_SVG from "../../assets/error.svg";
import DISCORD_IMG from "../../assets/discord.png";
import INSTAGRAM_IMG from "../../assets/instagram.png";
import TWITTER_IMG from "../../assets/twitter.png";
import PAUSE_IMG from "../../assets/pause.png";
//import RESUME_IMG from "../../assets/resume.png";

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
  width: 800,
  height: 530,
};

const errorWindowSize = {
  width: 800,
  height: 530,
};

const resizeWindow = async (size: WindowSize) => {
  const logicalSize = asLogicalSize(size);
  await getCurrentWindow().setSize(logicalSize).catch(console.error);
};

interface ChannelProxy {
  subscribe: (listener: (message: Status) => void) => void;
}

const newChannelProxy = () => {
  let currentChannel: Channel<Status> | null = null;
  let subscriber: ((message: Status) => void) | null = null;

  return {
    assignNewChannel: (channel: Channel<Status>) => {
      // remove previous listener
      if (currentChannel) currentChannel.onmessage = () => {};
      currentChannel = channel;
      currentChannel.onmessage = (arg) => {
        if (subscriber) subscriber(arg);
      };
    },
    subscribe: (listener: (message: Status) => void) => {
      subscriber = listener;
    },
  };
};

const useChannelUpdates = (channel: ChannelProxy) => {
  const [currentStatus, setCurrentStatus] = useState<Status | null>(null);
  useEffect(() => channel.subscribe(setCurrentStatus), [channel]);
  return currentStatus;
};

const channel = newChannelProxy();

export const Home: React.FC = memo(() => {
  const currentStatus = useChannelUpdates(channel);

  const rustCall = async (functionName: string) => {
    const newChannel = new Channel<Status>();
    channel.assignNewChannel(newChannel);
    await invoke(functionName, { channel: newChannel }).catch(console.error);
  };

  const launchFlow = async () => await rustCall("launch");
  const retryFlow = async () => await rustCall("retry");

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
                const message = "Downloading update...";
                if (data.data.progress)
                  return renderStep(message, {
                    message: "Downloading",
                    progress: data.data.progress,
                    bytesPerSecond: data.data.bytesPerSecond,
                    timeRemaining: data.data.timeRemaining,
                  });
                else return renderStep(message);
              }
              case "downloadFinished":
                return renderStep("Update downloaded...");
              case "installingUpdate":
                return renderStep("Installing update...");
              case "restartingApp":
                return renderStep("Restarting app...");
            }
          }
          case "deeplinkOpening":
            return renderDeeplinkOpeningStep();
          case "fetching":
            return renderFetchStep();
          case "downloading": {
            let data = currentStatus.data.step.data;
            let isUpdate = data.buildType === BuildType.Update;
            return renderDownloadStep(isUpdate, {
              message: "Downloading",
              progress: data.progress,
              bytesPerSecond: data.bytesPerSecond,
              timeRemaining: data.timeRemaining,
            });
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

  const renderDeeplinkOpeningStep = () => renderStep("Opening Deeplink...");

  const renderFetchStep = () => renderStep("Fetching Latest...");

  const renderDownloadStep = (isUpdate: boolean, progress: Progress) =>
    renderStep(
      isUpdate ? "Downloading Update..." : "Downloading Decentraland...",
      progress,
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
          <ErrorDialogButton variant="contained" onClick={retryFlow}>
            RETRY
          </ErrorDialogButton>
        </Box>
      </Box>
    );
  };

  // Generated by Claude.
  const humanReadableDownloadSpeed = (bytesPerSecond: number): string => {
    if (bytesPerSecond >= 1_000_000_000)
      return `${(bytesPerSecond / 1_000_000_000).toFixed(1)} GB/s`;
    if (bytesPerSecond >= 1_000_000)
      return `${(bytesPerSecond / 1_000_000).toFixed(1)} MB/s`;
    if (bytesPerSecond >= 1_000)
      return `${(bytesPerSecond / 1_000).toFixed(1)} KB/s`;
    return `${Math.round(bytesPerSecond)} B/s`;
  };

  // Generated by Claude.
  const humanReadableTimeRemaining = (timeRemaining: number): string => {
    const totalSeconds = Math.floor(timeRemaining / 1000);
    const hours = Math.floor(totalSeconds / 3600);
    const minutes = Math.floor((totalSeconds % 3600) / 60);
    const seconds = totalSeconds % 60;

    const parts: string[] = [];
    if (hours > 0) parts.push(`${hours} hr${hours !== 1 ? "s" : ""}`);
    if (minutes > 0) parts.push(`${minutes} min${minutes !== 1 ? "s" : ""}`);
    if (seconds > 0) parts.push(`${seconds} sec${seconds !== 1 ? "s" : ""}`);

    return parts.slice(0, 2).join(", ") || "0 sec";
  };

  const renderStep = (
    message: string,
    progress: Progress | undefined = undefined,
  ) => {
    resizeWindow(stateWindowSize);
    return (
      <Box marginTop="20px" alignSelf="center" display="flex" gap="14px">
        <Logo src={LOGO_SVG} />
        <Box display="flex" flexDirection="column" gap="2px">
          <Typography variant="h4" marginTop="4px" marginBottom="4px">
            {message}
          </Typography>
          <LoadingBar
            variant={progress ? "determinate" : undefined}
            value={progress ? progress.progress : undefined}
          />
          {progress && (
            <Box paddingTop="4px" paddingBottom="4px">
              <Box display="flex">
                <Typography flexGrow="1" fontWeight="600">
                  {progress.message}
                </Typography>
                <Typography fontWeight="600">{progress.progress}%</Typography>
              </Box>
              <Box display="flex">
                <Typography flexGrow="1" fontSize="12px">
                  {humanReadableDownloadSpeed(progress.bytesPerSecond)}
                </Typography>
                {progress.timeRemaining && (
                  <Typography>
                    {humanReadableTimeRemaining(progress.timeRemaining)}
                  </Typography>
                )}
              </Box>
            </Box>
          )}
        </Box>
        <IconButton>
          <img src={PAUSE_IMG} />
        </IconButton>
      </Box>
    );
  };

  return (
    <Box
      display="flex"
      flexDirection="column"
      flexGrow="1"
      sx={{
        backgroundImage: `url(${LANDSCAPE_IMG})`,
        backgroundPosition: "bottom",
      }}
    >
      <Box display="flex" flexDirection="column" flexGrow="1">
        {renderStatusMessage()}
      </Box>
      <Box display="flex" height="26px" overflow="hidden">
        <Typography alignSelf="center" flexGrow="1" marginLeft="10px">
          {versionLabel()}
        </Typography>
        <IconButton>
          <img src={DISCORD_IMG} />
        </IconButton>
        <IconButton>
          <img src={TWITTER_IMG} />
        </IconButton>
        <IconButton>
          <img src={INSTAGRAM_IMG} />
        </IconButton>
      </Box>
    </Box>
  );
});
