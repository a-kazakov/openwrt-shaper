import React from "react";
import ReactDOM from "react-dom/client";
import { ConfigProvider, theme } from "antd";
import App from "./App";

const root = document.getElementById("root")!;

ReactDOM.createRoot(root).render(
  <React.StrictMode>
    <ConfigProvider
      theme={{
        algorithm: theme.darkAlgorithm,
        token: {
          colorPrimary: "#ffffff",
          colorBgContainer: "#111111",
          colorBgElevated: "#111111",
          colorBgLayout: "#000000",
          colorBorder: "#222222",
          colorBorderSecondary: "#222222",
          colorText: "#ffffff",
          colorTextSecondary: "#999999",
          colorSuccess: "#4ade80",
          colorWarning: "#fbbf24",
          colorError: "#ef4444",
          borderRadius: 8,
          fontFamily:
            "-apple-system, BlinkMacSystemFont, 'SF Pro Text', 'Helvetica Neue', sans-serif",
        },
        components: {
          Card: {
            colorBgContainer: "#111111",
          },
          Table: {
            colorBgContainer: "#111111",
            headerBg: "#0a0a0a",
          },
          Drawer: {
            colorBgElevated: "#111111",
          },
          Button: {
            colorPrimary: "#ffffff",
            colorPrimaryHover: "#cccccc",
            colorPrimaryActive: "#aaaaaa",
            colorPrimaryText: "#000000",
            primaryColor: "#000000",
          },
          Progress: {
            colorSuccess: "#4ade80",
          },
          Tag: {
            colorBgContainer: "transparent",
          },
          InputNumber: {
            colorBgContainer: "#0a0a0a",
          },
          Input: {
            colorBgContainer: "#0a0a0a",
          },
        },
      }}
    >
      <App />
    </ConfigProvider>
  </React.StrictMode>,
);
