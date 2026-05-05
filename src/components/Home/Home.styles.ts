import { styled, LinearProgress } from "decentraland-ui2";

export const Logo = styled("img")((props) => ({
  ...props,
  height: "50px",
  width: "50px",
}));

export const ErrorIcon = styled("img")((props) => ({
  ...props,
  height: "50px",
  width: "50px",
}));

export const LoadingBar = styled(LinearProgress)((props) => ({
  ...props,
  width: "348px",
  height: "7px",
  backgroundColor: "rgba(255, 255, 255, 0.1)",
  borderRadius: "3.5px",
  "& .MuiLinearProgress-bar": {
    background: "linear-gradient(90deg, #FF2D55 0%, #FFBC5B 100%)",
    borderRadius: "3.5px",
  },
}));
