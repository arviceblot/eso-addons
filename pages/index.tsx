import type { NextPage } from 'next'
import Head from 'next/head'
import Image from 'next/image'
import { invoke } from "@tauri-apps/api/tauri"
import { useEffect } from 'react'

const Home: NextPage = () => {
  useEffect(() => {
    invoke('get_installed_addon_count')
      .then(console.log)
      .catch(console.error)
  }, []);

  function handleUpdateButton() {
    invoke('update').then(console.log).catch(console.error);
  }

  return (
    <div>
      <Head>
        <title>ESO Addon Manager</title>
        <meta name="description" content="Manage ESO addons" />
        <link rel="icon" href="/favicon.ico" />
      </Head>

      <main>
        <div className="flex mb-6 px-4 py-2 rounded-xl text-text">
          <a href="#">
            <button className="bg-lavender hover:bg-mauve active:bg-mauve/75" onClick={handleUpdateButton}>Update</button>
          </a>
        </div>
        <div>
          <ul>

          </ul>
        </div>
      </main>
    </div>
  )
}

export default Home
