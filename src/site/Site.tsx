import * as React from 'react'

import DmgConsole from './pages/DmgConsole'
import SgbConsole from './pages/SgbConsole'
import MgbConsole from './pages/MgbConsole'
import MglConsole from './pages/MglConsole'
import Sgb2Console from './pages/Sgb2Console'
import CgbConsole from './pages/CgbConsole'
import AgbConsole from './pages/AgbConsole'
import AgsConsole from './pages/AgsConsole'
import GbsConsole from './pages/GbsConsole'
import OxyConsole from './pages/OxyConsole'
import SiteFooter from './components/SiteFooter'
import SiteHeader from './components/SiteHeader'
import Cartridge from './pages/Cartridge'
import Game from './pages/Game'
import Cartridges from './pages/Cartridges'
import Mapper from './pages/Mapper'

namespace Site {
  export interface Props {
    pageType: string
    title: string
    pageProps: any
    consoleSubmissionCount: number
    cartridgeSubmissionCount: number
  }
}

export default function Site(props: Site.Props) {
  return (
    <html lang="en">
      <head>
        <meta charSet="utf-8" />
        <meta httpEquiv="X-UA-Compatible" content="IE=edge" />
        <meta name="viewport" content="width=device-width, initial-scale=1" />
        <title>{props.title}</title>
        <link rel="stylesheet" href="//fonts.googleapis.com/css?family=Lato:400,700" />
        <link rel="stylesheet" href="/static/gbhwdb.css" />
        <link rel="apple-touch-icon" sizes="180x180" href="/apple-touch-icon.png" />
        <link rel="icon" type="image/png" sizes="32x32" href="/favicon-32x32.png" />
        <link rel="icon" type="image/png" sizes="16x16" href="/favicon-16x16.png" />
        <link rel="manifest" href="/site.webmanifest" />
        <link rel="mask-icon" href="/safari-pinned-tab.svg" color="#5bbad5" />
        <meta name="msapplication-TileColor" content="#2b5797" />
        <meta name="theme-color" content="#ffffff" />
        <script dangerouslySetInnerHTML={{ __html: googleAnalytics() }} />
        <script async src="https://www.google-analytics.com/analytics.js" />
      </head>
      <body>
        <SiteHeader pageType={props.pageType} />
        <main className="site-main">
          <div className="site-main__content">{renderPage(props.pageType, props.pageProps)}</div>
        </main>
        <SiteFooter
          consoleSubmissionCount={props.consoleSubmissionCount}
          cartridgeSubmissionCount={props.cartridgeSubmissionCount}
        />
      </body>
    </html>
  )
}

function renderPage(type: string, props: any) {
  switch (type) {
    case 'dmg-console':
      return <DmgConsole {...props} />
    case 'sgb-console':
      return <SgbConsole {...props} />
    case 'mgb-console':
      return <MgbConsole {...props} />
    case 'mgl-console':
      return <MglConsole {...props} />
    case 'sgb2-console':
      return <Sgb2Console {...props} />
    case 'cgb-console':
      return <CgbConsole {...props} />
    case 'agb-console':
      return <AgbConsole {...props} />
    case 'ags-console':
      return <AgsConsole {...props} />
    case 'gbs-console':
      return <GbsConsole {...props} />
    case 'oxy-console':
      return <OxyConsole {...props} />
    case 'cartridges':
      return <Cartridges {...props} />
    case 'cartridge':
      return <Cartridge {...props} />
    case 'game':
      return <Game {...props} />
    case 'mapper':
      return <Mapper {...props} />
    default:
      return null
  }
}

function googleAnalytics() {
  return `window.ga=window.ga||function(){(ga.q=ga.q||[]).push(arguments)};ga.l=+new Date;
  ga('create', 'UA-37123121-2', 'auto');
  ga('send', 'pageview');`
}
