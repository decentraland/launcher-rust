import { useEffect, useState } from "react";
import { Typography } from "decentraland-ui2";
import { getVersion } from "@tauri-apps/api/app";

export function versionLabel() {
  const [version, setVersion] = useState<string>("");

  useEffect(() => {
    getVersion()
      .then(v => setVersion(`v${v}`))
      .catch(() => setVersion("v?.?.?"));
  }, []);

  return (
    <Typography
      align="left"
      sx={{
        position: "fixed",
        bottom: "8px",
        left: "12px",
        fontFamily: "Inter, sans-serif",
        fontSize: "12px",
        fontWeight: 600,
        verticalAlign: "bottom",
        color: "rgba(255, 255, 255, 0.5)",
      }}
    >
      {version}
    </Typography>
  );
}
