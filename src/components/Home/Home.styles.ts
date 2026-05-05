import { styled, LinearProgress, Button } from "decentraland-ui2";

const iconSideSize = "50px";

export const Logo = styled("img")((props) => ({
  ...props,
  height: iconSideSize,
  width: iconSideSize,
}));

export const ErrorIcon = styled("img")((props) => ({
  ...props,
  height: iconSideSize,
  width: iconSideSize,
}));

export const ErrorDialogButton = styled(Button)((props) => ({
  ...props,
  width: "190px",
  height: "46px",
  borderRadius: "12px",
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

export const SocialButton = styled("div")({
  width: "22px",
  height: "22px",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
});
