import React from "react";
import { Home } from "./components/Home/Home";
import { Box } from "decentraland-ui2";
import { ThemeProvider } from "decentraland-ui2/dist/theme";
import { Theme } from "./theme";

export const App: React.FC = () => {
  return (
    //<React.StrictMode>
      <ThemeProvider theme={Theme}>
        <Box
          display="flex"
          height="100vh"
        >
          <Home />
        </Box>
      </ThemeProvider>
    //</React.StrictMode>
  );
};
