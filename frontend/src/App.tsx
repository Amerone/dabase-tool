import { RouterProvider } from 'react-router-dom'
import { ConfigProvider, theme } from 'antd'
import zhCN from 'antd/locale/zh_CN'
import { router } from '@/router'
import 'antd/dist/reset.css'

function App() {
  return (
    <ConfigProvider
      locale={zhCN}
      theme={{
        algorithm: theme.darkAlgorithm,
        token: {
          colorPrimary: '#14c8be',
          colorInfo: '#4aa8ff',
          colorBgBase: '#070d18',
          colorBgContainer: 'rgba(14, 23, 37, 0.86)',
          colorBgElevated: 'rgba(19, 30, 47, 0.95)',
          colorBorder: 'rgba(80, 145, 255, 0.28)',
          colorText: '#ecf2ff',
          colorTextSecondary: '#9cb2d4',
          borderRadius: 8,
          fontFamily: '"IBM Plex Sans", "PingFang SC", "Microsoft YaHei", sans-serif',
          fontFamilyCode: '"JetBrains Mono", monospace',
        },
        components: {
          Card: {
            colorBgContainer: 'rgba(14, 24, 40, 0.7)',
            colorBorderSecondary: 'rgba(105, 162, 255, 0.24)',
            boxShadowTertiary: '0 24px 50px rgba(0, 8, 22, 0.46)',
          },
          Button: {
            borderRadius: 8,
            controlHeight: 40,
            defaultBorderColor: 'rgba(104, 150, 243, 0.45)',
            defaultBg: 'rgba(25, 39, 62, 0.72)',
            defaultColor: '#d9e8ff',
            primaryShadow: '0 14px 30px rgba(20, 200, 190, 0.32)',
          },
          Input: {
            colorBgContainer: 'rgba(13, 22, 37, 0.9)',
            activeBorderColor: '#14c8be',
            hoverBorderColor: '#4aa8ff',
            borderRadius: 8,
          },
          Table: {
            colorBgContainer: 'rgba(9, 17, 31, 0.45)',
            headerBg: 'rgba(60, 102, 186, 0.16)',
            rowHoverBg: 'rgba(72, 164, 255, 0.12)',
          },
          Collapse: {
            headerBg: 'rgba(10, 18, 30, 0.6)',
            contentBg: 'rgba(8, 14, 26, 0.36)',
          },
        },
      }}
    >
      <RouterProvider router={router} />
    </ConfigProvider>
  )
}

export default App
