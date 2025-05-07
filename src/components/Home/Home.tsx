import React, { memo, useCallback, useEffect, useState } from 'react';
import { Box, Button, Typography } from 'decentraland-ui2';
import { Status, BuildType } from './types';
import { Landscape, LoadingBar, Logo } from './Home.styles';
import LANDSCAPE_IMG from '../../assets/background.jpg';
import LOGO_SVG from '../../assets/logo.svg';
import { invoke, Channel } from '@tauri-apps/api/core';

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
    await invoke('launch', { channel }).catch(console.error);
  };

  useEffect(() => {
    launchFlow();
  }, []);

  const renderStatusMessage = () => {
    if (!currentStatus) return null;

    switch (currentStatus.event) {
      case 'state':
        switch (currentStatus.data.step.event) {
          case 'fetching':
            return renderFetchStep();
          case 'downloading':
            {
              let data = currentStatus.data.step.data;
              let isUpdate = data.buildType === BuildType.Update;
              let progress = data.progress;
              return renderDownloadStep(isUpdate, progress);
            }
          case 'installing':
            let data = currentStatus.data.step.data;
            let isUpdate = data.buildType === BuildType.Update;
            return renderInstallStep(isUpdate);
          case 'launching':
            return renderLaunchStep();
        }
      case 'error':
        return renderError(currentStatus.data.canRetry, currentStatus.data.message);
      default:
        return null;
    }
  };

  const renderFetchStep = () =>
    renderStep('Fetching Latest...')

  const renderDownloadStep = (isUpdate: boolean, downloadingProgress: number) =>
    renderStep(isUpdate ? 'Downloading Update...' : 'Downloading Decentraland...', downloadingProgress);

  const renderInstallStep = (isUpdate: boolean) =>
    renderStep(isUpdate ? 'Installing Update...' : 'Installation in Progress...');

  const renderLaunchStep = () =>
    renderStep('Launching Decentraland...');

  const renderError = useCallback((shouldShowRetryButton: boolean, message: string) => {
    message += '...';
    if (shouldShowRetryButton) {
      return (
        <Box>
          <Typography variant="h4" align="center">
            {message}
          </Typography>
          <Box display="flex" justifyContent="center" marginTop={'10px'}>
            <Button onClick={launchFlow}>Retry</Button>
          </Box>
        </Box>
      );
    }

    return (
      <Box>
        <Typography variant="h4" align="center">
          Retrying...
        </Typography>
      </Box>
    );
  }, []);

  const renderStep = (message: string, downloadingProgress: number | undefined = undefined) => {
    return (
      <Box display="flex" flexDirection="column" justifyContent="space-between" height="61px">
        <Typography variant="h6" align="left" fontWeight="bold" >
          {message}
        </Typography>
        <Box sx={{ display: "flex", alignItems: "center", justifyContent: "center" }}>
          <LoadingBar variant={downloadingProgress ? "determinate" : undefined} value={downloadingProgress ?? undefined} sx={{ mr: 1 }} />
          {<Typography variant="body1" width="45px" visibility={downloadingProgress ? "visible" : "hidden"}>{`${Math.round(downloadingProgress ?? 0)}%`}</Typography>}
        </Box>
      </Box>
    );
  };

  return (
    <Box display="flex" alignItems={'center'} justifyContent={'center'} width={'100%'} gap={3.5}>
      <Landscape>
        <img src={LANDSCAPE_IMG} />
      </Landscape>
      <Logo src={LOGO_SVG} />
      {renderStatusMessage()}
    </Box>
  );
});