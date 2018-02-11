import * as React from 'react';

import * as config from '../../config';

interface Props {
  pageType: string;
}

export default function SiteHeader({pageType}: Props) {
  return (
    <header className="site-header">
      <h1 className="site-header__title">
        <a href="/">
          Game Boy hardware database
          <aside>by Gekkio and contributors</aside>
        </a>
      </h1>
      <Navigation pageType={pageType} />
    </header>
  )
}

const models = config.consoles.map(type => [type.toUpperCase(), type, config.consoleCfgs[type].name])

function isModel(pageType: string, code: string) {
  return pageType === code || pageType === `${code}-console`
}

function Navigation({pageType}: Props) {
  return (
    <nav className="site-navigation">
      <ul>{
        models.map(([model, code, name]) => (
          <li key={code} className={(isModel(pageType, code)) ? 'active' : undefined}>
            <a href={`/consoles/${code}`}>
              <strong>{model}</strong>
              <span className="name">{name}</span>
            </a>
          </li>
        ))
      }</ul>
    </nav>
  )
}
