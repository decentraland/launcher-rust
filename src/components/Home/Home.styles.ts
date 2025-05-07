import { styled, LinearProgress, Button } from 'decentraland-ui2';

export const Landscape = styled('div')(_props => ({
  position: 'absolute',
  top: 0,
  left: 0,
  bottom: 0,
  width: '100%',
  height: '100%',
  overflow: 'hidden',
  zIndex: -1,
  '::after': {
    position: 'absolute',
    top: 0,
    left: 0,
    bottom: 0,
    width: '100%',
    height: '100%',
    content: "''",
    mixBlendMode: 'multiply',
    pointerEvents: 'none',
  },
  img: {
    width: '100%',
    height: '100%',
    objectFit: 'cover',
  },
}));

export const Logo = styled('img')(props => ({
  ...props,
  height: '61px',
  width: '61px',
}));

export const ErrorIcon = styled('img')(props => ({
  ...props,
  height: '62px',
  width: '62px',
}));

export const ErrorDialogButton = styled(Button)(props => ({
  ...props,
  width: '190px',
  height: '46px',
}));


export const LoadingBar = styled(LinearProgress)(props => ({
  ...props,
  width: '348px',
  height: '7px',
  backgroundColor: 'rgba(255, 255, 255, 0.1)',
  borderRadius: '3.5px',
  '& .MuiLinearProgress-bar': {
    background: 'linear-gradient(90deg, #FF2D55 0%, #FFBC5B 100%)',
    borderRadius: '3.5px'
  }
}));
