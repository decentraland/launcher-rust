import "@fontsource/inter";
import { deepmerge } from "@mui/utils";
import { dark } from "decentraland-ui2/dist/theme";

export const Theme = deepmerge(dark, {
  typography: {
    fontFamily: "Inter",
    body1: {
      fontSize: "12px",
    },
    h4: {
      fontSize: "20px",
      fontWeight: "600",
    },
    h6: {
      fontSize: "16px",
    },
  },
});
