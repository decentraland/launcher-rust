import React, { memo, useCallback, useEffect, useState } from 'react';
import { Box, Button, Typography } from 'decentraland-ui2';
import { Status, BuildType } from './types';
import { Landscape, LoadingBar } from './Home.styles';
import LANDSCAPE_IMG from '../../assets/landscape.png';
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
  //  const shouldRunDevVersion = getRunDevVersion();
  //const customDownloadedFilePath = getDownloadedFilePath();

  useEffect(() => { launchFlow(); }, []);

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
        break;
      case 'error':
        return renderError(currentStatus.data.canRetry, currentStatus.data.message);
      default:
        return null;
    }
  };

  const renderFetchStep = useCallback(() => {
    return <Typography variant="h4">Fetching the latest available version of Decentraland</Typography>;
  }, []);

  const renderDownloadStep = useCallback((isUpdate: boolean, downloadingProgress: number) => {
    return (
      <Box>
        <Typography variant="h4" align="center">
          {isUpdate ? 'Downloading Update' : 'Downloading Decentraland'}
        </Typography>
        <Box sx={{ display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
          <LoadingBar variant="determinate" value={downloadingProgress} sx={{ mr: 1 }} />
          <Typography variant="body1">{`${Math.round(downloadingProgress)}%`}</Typography>
        </Box>
      </Box>
    );
  }, []);

  const renderInstallStep = useCallback((isUpdate: boolean) => {
    return (
      <Box>
        <Typography variant="h4" align="center">
          {isUpdate ? 'Installing Update' : 'Installation in Progress'}
        </Typography>
        <Box paddingTop={'10px'} paddingBottom={'10px'}>
          <LoadingBar />
        </Box>
      </Box>
    );
  }, []);

  const renderLaunchStep = useCallback(() => {
    return <Typography variant="h4">Launching Decentraland</Typography>;
  }, []);

  const handleOnClickRetry = useCallback(() => {
      launchFlow();
  }, []);

  const renderError = useCallback((shouldShowRetryButton: boolean, message: string) => {
    if (shouldShowRetryButton) {

      return (
        <Box>
          <Typography variant="h4" align="center">
            {
                message
               /* TODO provide corresponding descriptions from rust side
               * isFetching
              ? 'Fetch the latest client version failed'
              : isDownloading
                ? 'Download failed'
                : isInstalling
                  ? 'Install failed'
                  : 'Error'
                  */
            }

          </Typography>
          <Typography variant="body1" align="center">
            {
          /* TODO
                 isFetching || isDownloading
              ? 'Please check your internet connection and try again.'
              : isInstalling
                ? 'Please try again.'
                : error
        */
            }
          </Typography>
          <Box display="flex" justifyContent="center" marginTop={'10px'}>
            <Button onClick={handleOnClickRetry}>Retry</Button>
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

  return (
    <Box display="flex" alignItems={'center'} justifyContent={'center'} width={'100%'}>
      <Landscape>
        <img src={LANDSCAPE_IMG} />
      </Landscape>
      {renderStatusMessage()}
    </Box>
  );
});
