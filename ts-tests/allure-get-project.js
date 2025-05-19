// import fs from "fs";
// import path from "path"
// import FormData from "form-data";
// import glob from "glob-promise";
// import fetch from "node-fetch";


// Login if needed
let csrfAccessToken;
let cookies = [];
console.log("Logging in...");

const allureServerUrl="https://allure-server.prd.midnight.tools/"
const securityUser="admin"
const securityPass="the.best.things.are.yet.to.come"
const projectId="indexer-rs-it-testnet-02"

const loginResponse = await fetch(
    `${allureServerUrl}allure-docker-service/login`,
    {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({
            username: securityUser,
            password: securityPass,
        }),
    }
);

if (loginResponse.ok) {
    console.log("Done logging in...");
    const loginCookies = loginResponse.headers.raw()["set-cookie"];
    const csrfAccessTokenPattern = /csrf_access_token=([^;]+)/;
    for (const cookie of loginCookies) {
        const match = cookie.match(csrfAccessTokenPattern);
        if (match && match[1]) {
            csrfAccessToken = match[1];
            break;
        }
    }
    cookies = loginCookies.join("; ");
} else {
    throw Error(
        `Status code of login was: ${loginResponse.statusCode
        } Body: ${await loginResponse.text()}`
    );
}


// Fetch latest report ID
console.log("Getting report link...");
const latestReportResponse = await fetch(
    `${allureServerUrl}allure-docker-service/projects/${projectId}`,
    { method: "GET", headers: { Cookie: cookies } }
);
const latestReportBody = await latestReportResponse.json();
if (latestReportResponse.ok) {
    console.log("Done getting report link...");
    const latestReportId = latestReportBody.data.project.reports_id[1];
    const reportLink = `${allureServerUrl}allure-docker-service-ui/projects/${projectId}/reports/${parseInt(latestReportId) + 1
        }`;
    console.log("Allure Report Link:", reportLink);
} else {
    throw Error(
        `Failed to fetch latest report ID. Status code: ${latestReportResponse.statusCode} Body: ${latestReportBody}`
    );
}