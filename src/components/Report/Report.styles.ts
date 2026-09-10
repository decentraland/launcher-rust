import {
  styled,
  Button,
  Typography,
  TextField,
  Select,
} from "decentraland-ui2";

export const PRIMARY = "#FF2D55";
const TEXT = "#FFFFFF";
const INPUT_TEXT = "#1B1B1F";
const FONT = "Inter, sans-serif";

// Fills the window so the whole launcher reads as the dialog from the design.
export const Panel = styled("div")({
  position: "fixed",
  inset: 0,
  zIndex: 1,
  boxSizing: "border-box",
  padding: "40px 48px 32px",
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  overflowY: "auto",
  color: TEXT,
  fontFamily: FONT,
  background: "linear-gradient(180deg, #4E1480 0%, #340A5C 100%)",
});

// Icon + title sit on a soft lighter-purple highlight, not on a badge.
export const Header = styled("div")({
  display: "flex",
  flexDirection: "column",
  alignItems: "center",
  padding: "8px 48px 4px",
  marginBottom: 12,
  background:
    "radial-gradient(ellipse 60% 70% at 50% 45%, rgba(150, 70, 220, 0.55) 0%, rgba(150, 70, 220, 0) 100%)",
  // White outer outline around the dark stroke, as in the design's sticker-style icons. A
  // four-direction zero-blur drop-shadow outlines the whole silhouette, multi-shape icons too.
  "& svg": {
    display: "block",
    marginBottom: 12,
    filter:
      "drop-shadow(2px 0 0 #fff) drop-shadow(-2px 0 0 #fff) drop-shadow(0 2px 0 #fff) drop-shadow(0 -2px 0 #fff)",
  },
});

export const Title = styled(Typography)({
  fontFamily: FONT,
  fontWeight: 700,
  fontSize: 20,
  lineHeight: "28px",
  textAlign: "center",
  color: TEXT,
});

export const Body = styled(Typography)({
  fontFamily: FONT,
  fontSize: 15,
  lineHeight: "22px",
  textAlign: "center",
  color: TEXT,
  maxWidth: 440,
});

export const Field = styled("div")({
  width: "100%",
  maxWidth: 560,
  textAlign: "left",
  marginBottom: 24,
});

export const FieldLabel = styled(Typography)({
  fontFamily: FONT,
  fontWeight: 700,
  fontSize: 12,
  letterSpacing: "0.6px",
  textTransform: "uppercase",
  color: TEXT,
  "& span": { color: PRIMARY },
});

export const FieldHint = styled(Typography)({
  fontFamily: FONT,
  fontSize: 14,
  color: TEXT,
  marginBottom: 8,
});

export const IssueSelect = styled(Select)({
  width: "100%",
  height: 46,
  borderRadius: 10,
  backgroundColor: "#FFFFFF",
  color: INPUT_TEXT,
  fontFamily: FONT,
  fontWeight: 600,
  "& .MuiOutlinedInput-notchedOutline": { border: "none" },
  "& .MuiSelect-icon": { color: INPUT_TEXT },
});

export const DescriptionField = styled(TextField)({
  width: "100%",
  "& .MuiOutlinedInput-root": {
    borderRadius: 10,
    backgroundColor: "#FAFAFA",
    color: INPUT_TEXT,
    fontFamily: FONT,
    "& fieldset": { border: "none" },
  },
  "& .MuiInputBase-input::placeholder": { color: "#8A8A93", opacity: 1 },
});

export const ErrorText = styled(Typography)({
  fontFamily: FONT,
  fontSize: 14,
  color: "#FF8FA3",
  textAlign: "center",
  marginTop: 8,
});

export const ButtonRow = styled("div")({
  display: "flex",
  gap: 16,
  width: "100%",
  maxWidth: 560,
  marginTop: 24,
  justifyContent: "center",
});

// Four classes and the `background` shorthand: the theme's contained-button rules use three.
const baseButton = {
  flex: 1,
  height: 46,
  borderRadius: 12,
  fontFamily: FONT,
  fontWeight: 700,
  fontSize: 14,
  letterSpacing: "0.4px",
  color: TEXT,
  boxShadow: "none",
};

export const PrimaryButton = styled(Button)({
  "&&&&": {
    ...baseButton,
    background: PRIMARY,
  },
  "&&&&:hover": { background: "#E5284C", boxShadow: "none" },
  "&&&&.Mui-disabled": {
    background: "rgba(255, 45, 85, 0.45)",
    color: "rgba(255, 255, 255, 0.7)",
  },
});

export const SecondaryButton = styled(Button)({
  "&&&&": {
    ...baseButton,
    background: "#2B0B45",
  },
  "&&&&:hover": { background: "#1F0733", boxShadow: "none" },
  "&&&&.Mui-disabled": {
    background: "rgba(43, 11, 69, 0.6)",
    color: "rgba(255, 255, 255, 0.6)",
  },
});
