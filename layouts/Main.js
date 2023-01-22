import React from "react";

// components

import MainNavbar from "../components/Navbars/MainNavbar";
// import HeaderStats from "components/Headers/HeaderStats.js";

export default function Admin({ children }) {
    return (
        <>
            <div className="relative w-full h-full bg-gradient-to-b from-base to-crust py-40 min-h-screen">
                <MainNavbar />
                {/* Header */}
                {/* <HeaderStats /> */}
                <div className="px-4 md:px-10 mx-auto w-full -m-24">
                    {children}
                </div>
            </div>
        </>
    );
}