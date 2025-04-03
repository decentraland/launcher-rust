use std::fmt;
use std::fmt::Display;

pub enum Event {
    LAUNCHER_OPEN {
        version: String
    }, 
    LAUNCHER_CLOSE {
        version: String
    },
    DOWNLOAD_VERSION {
        version: String
    },
    DOWNLOAD_VERSION_SUCCESS {
        version: String
    },
    DOWNLOAD_VERSION_ERROR {
        version: Option<String>,
        error: String,
    },
    DOWNLOAD_VERSION_CANCELLED {
        version: String
    },
    INSTALL_VERSION_START {
        version: String
    },
    INSTALL_VERSION_SUCCESS {
        version: String
    }, 
    INSTALL_VERSION_ERROR {
        version: String
    },
    LAUNCH_CLIENT_START {
        version: String
    },
    LAUNCH_CLIENT_SUCCESS {
        version: String
    }, 
    LAUNCH_CLIENT_ERROR {
        version: String
    },
    LAUNCHER_UPDATE_CHECKING,
    LAUNCHER_UPDATE_AVAILABLE {
        version: String
    },
    LAUNCHER_UPDATE_NOT_AVAILABLE,
    LAUNCHER_UPDATE_CANCELLED {
        version: String
    },
    LAUNCHER_UPDATE_ERROR {
        version: String,
        error: String,
    },
    LAUNCHER_UPDATE_DOWNLOADED {
        version: String
    },
}

impl Display for Event {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", match self {
            Event::LAUNCHER_OPEN {..} => "Launcher Open",
            Event::LAUNCHER_CLOSE {..} => "Launcher Close",
            Event::DOWNLOAD_VERSION {..} => "Download Version",
            Event::DOWNLOAD_VERSION_SUCCESS {..} => "Download Version Success",
            Event::DOWNLOAD_VERSION_ERROR {..} => "Download Version Error",
            Event::DOWNLOAD_VERSION_CANCELLED {..} => "Download Version Cancelled",
            Event::INSTALL_VERSION_START {..} => "Install Version Start",
            Event::INSTALL_VERSION_SUCCESS {..} => "Install Version Success",
            Event::INSTALL_VERSION_ERROR {..} => "Install Version Error",
            Event::LAUNCH_CLIENT_START {..} => "Launch Client Start",
            Event::LAUNCH_CLIENT_SUCCESS {..} => "Launch Client Success",
            Event::LAUNCH_CLIENT_ERROR {..} => "Launch Client Error",
            Event::LAUNCHER_UPDATE_CHECKING {..} => "Launcher Update Checking",
            Event::LAUNCHER_UPDATE_AVAILABLE {..} => "Launcher Update Available",
            Event::LAUNCHER_UPDATE_NOT_AVAILABLE {..} => "Launcher Update Not Available",
            Event::LAUNCHER_UPDATE_CANCELLED {..} => "Launcher Update Cancelled",
            Event::LAUNCHER_UPDATE_ERROR {..} => "Launcher Update Error",
            Event::LAUNCHER_UPDATE_DOWNLOADED {..} => "Launcher Update Downloaded",
        })
    }
}
