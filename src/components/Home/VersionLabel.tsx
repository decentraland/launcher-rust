import { useEffect, useState } from "react";
import { getVersion } from "@tauri-apps/api/app";

export function versionLabel() {
  const [version, setVersion] = useState<string>("");

  useEffect(() => {
    getVersion()
      .then((v) => setVersion(`v${v}`))
      .catch(() => setVersion("v?.?.?"));
  }, []);

  return (
    <>
      {version}
    </>
  );
}
