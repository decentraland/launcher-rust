import { styled, LinearProgress } from 'decentraland-ui2';

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
    backgroundColor: 'rgba(0, 0, 0, 0.8)',
    mixBlendMode: 'multiply',
    pointerEvents: 'none',
  },
  img: {
    width: '100%',
    height: '100%',
    objectFit: 'cover',
  },
}));

export const LoadingBar = styled(LinearProgress)(props => ({
  ...props,
  width: '450px',
}));
