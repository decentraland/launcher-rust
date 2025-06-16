import React from "react";
import { Home } from "./components/Home/Home";
import { Container } from "decentraland-ui2";
import { ThemeProvider } from "decentraland-ui2/dist/theme";
import { Theme } from "./theme";

export const App: React.FC = () => {
  return (
    <React.StrictMode>
      <ThemeProvider theme={Theme}>
        <Container
          id="app-container"
          fixed
          sx={{ display: "flex", height: "100vh" }}
        >
          <Home />
        </Container>
      </ThemeProvider>
    </React.StrictMode>
  );
};
