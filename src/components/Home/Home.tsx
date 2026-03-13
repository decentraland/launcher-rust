import React, { memo, useEffect, useState } from "react";
import { Box, Typography, IconButton } from "decentraland-ui2";
import { Status, BuildType } from "./types";
import {
  Landscape,
  LoadingBar,
  Logo,
  ErrorIcon,
  ErrorDialogButton,
} from "./Home.styles";
import { versionLabel } from "./VersionLabel";

import LANDSCAPE_IMG from "../../assets/background.jpg";
import LOGO_SVG from "../../assets/logo.svg";
import ERROR_SVG from "../../assets/error.svg";
import DISCORD_IMG from "../../assets/discord.png"
import INSTAGRAM_IMG from "../../assets/instagram.png"
import TWITTER_IMG from "../../assets/twitter.png"
import PAUSE_IMG from "../../assets/pause.png"
import RESUME_IMG from "../../assets/resume.png"

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
                return renderStep("Checking for update...", "Checking");
              case "downloading": {
                const progress = data.data.progress ?? undefined;
                return renderStep("Downloading update...", "Downloading", progress);
              }
              case "downloadFinished":
                return renderStep("Update downloaded...", "Downloading");
              case "installingUpdate":
                return renderStep("Installing update...", "Installing");
              case "restartingApp":
                return renderStep("Restarting app...", "Restarting");
            }
          }
          case "deeplinkOpening":
            return renderDeeplinkOpeningStep();
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

  const renderDeeplinkOpeningStep = () => renderStep("Opening Deeplink...", "Opening");

  const renderFetchStep = () => renderStep("Fetching Latest...", "Fetching");

  const renderDownloadStep = (isUpdate: boolean, downloadingProgress: number) =>
    renderStep(
      isUpdate ? "Downloading Update..." : "Downloading Decentraland...", "Downloading",
      downloadingProgress,
    );

  const renderInstallStep = (isUpdate: boolean) =>
    renderStep(
      isUpdate ? "Installing Update..." : "Installation in Progress...", "Installing",
    );

  const renderLaunchStep = () => renderStep("Launching Decentraland...", "Launching");

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

  const renderStep = (
    message: string,
    message2: string,
    downloadingProgress: number | undefined = undefined,
  ) => {
    resizeWindow(stateWindowSize);
    return (
      <Box
        marginTop="20px"
        alignSelf="center"
        display="flex"
        gap="14px"
      >
        <Logo
          src={LOGO_SVG}
        />
        <Box
          display="flex"
          flexDirection="column"
          gap="2px"
        >
          <Typography
            variant="h4"
            marginTop="4px"
            marginBottom="4px"
          >
            {message}
          </Typography>
          <LoadingBar
            variant={downloadingProgress ? "determinate" : undefined}
            value={downloadingProgress ?? undefined}
          />
          <Box
            paddingTop="4px"
            paddingBottom="4px"
          >
            <Box
              display="flex"
            >
              <Typography
                flexGrow="1"
                fontWeight="600"
              >
                {message2}
              </Typography>
              <Typography
                fontWeight="600"
              >
                {downloadingProgress}%
              </Typography>
            </Box>
            <Box display="flex">
              <Typography
                flexGrow="1"
                fontSize="12px"
              >
                10 MB/s
              </Typography>
              <Typography>
                45 minutes
              </Typography>
            </Box>
          </Box>
        </Box>
        <IconButton>
          <img src={PAUSE_IMG}/>
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
        backgroundPosition: "bottom"
      }}
    >
      <Box
        display="flex"
        flexDirection="column"
        flexGrow="1"
      >
        {renderStatusMessage()}
      </Box>
      <Box
        display="flex"
        height="26px"
        overflow="hidden"
      >
        <Typography
          alignSelf="center"
          flexGrow="1"
          marginLeft="10px"
        >
          {versionLabel()}
        </Typography>
        <IconButton>
          <img src={DISCORD_IMG}/>
        </IconButton>
        <IconButton>
          <img src={TWITTER_IMG}/>
        </IconButton>
        <IconButton>
          <img src={INSTAGRAM_IMG}/>
        </IconButton>
      </Box>
    </Box>
  );
});
