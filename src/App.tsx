import React from 'react';
import { Container } from 'decentraland-ui2';
import { dark, ThemeProvider } from 'decentraland-ui2/dist/theme'
import { Home } from './components/Home/Home';

export const App: React.FC = () => {
  return (
    <React.StrictMode>
      <ThemeProvider theme={dark}>
        <Container id="app-container" fixed sx={{ display: 'flex', height: '100vh' }}>
          <Home />
        </Container>
      </ThemeProvider>
    </React.StrictMode>
  );
};
