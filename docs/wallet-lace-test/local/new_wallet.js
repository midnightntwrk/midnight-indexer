let mnemonic = [];
const walletName = "fresh_sean3";
const walletPassword = "testPa$$word123";

const offset = [0, 8, 16];
const delay = (ms) => new Promise((res) => setTimeout(res, ms));

await (async () => {
  document.querySelectorAll("button")[1].click();
  await delay(300);
  document
    .querySelector('[data-testid="wallet-setup-legal-terms-checkbox"]')
    .click();
  await delay(300);
  document.querySelector('[data-testid="wallet-setup-step-btn-next"]').click();
  await delay(300);
  document
    .querySelector('[data-testid="wallet-setup-register-name-input"]')
    .focus();
  document.execCommand("insertText", "false", walletName);
  await delay(300);
  document.querySelector('[data-testid="wallet-setup-step-btn-next"]').click();
  await delay(300);
  document
    .querySelector('[data-testid="wallet-setup-password-step-password"]')
    .focus();
  document.execCommand("insertText", "false", walletPassword);
  await delay(300);
  document
    .querySelector(
      '[data-testid="wallet-setup-password-step-confirm-password"]'
    )
    .focus();
  document.execCommand("insertText", "false", walletPassword);
  await delay(300);
  document.querySelector('[data-testid="wallet-setup-step-btn-next"]').click();
  await delay(300);
  document
    .querySelector('[data-testid="network-undeployed-radio-button"]')
    .click();
  await delay(600);
  document.querySelector('[data-testid="wallet-setup-step-btn-next"]').click();
  await delay(300);
  document.querySelector('[data-testid="wallet-setup-step-btn-next"]').click();
  await delay(300);

  const divs = document.querySelectorAll('p[class*="MnemonicWordsWritedown"]');
  for (let k = 0; k < 3; k++) {
    for (let i = 0; i < 8; i++) {
      mnemonic.push(divs[i].textContent);
    }
    document
      .querySelector('[data-testid="wallet-setup-step-btn-next"]')
      .click();
    await delay(200);
  }
  await delay(800);
})();

(async () => {
  const inputs = document.querySelectorAll(".ant-input");
  for (let k = 0; k < 3; k++) {
    for (let i = 0; i < 8; i++) {
      inputs[i].focus();
      document.execCommand("insertText", "false", mnemonic[i + offset[k]]);
      await delay(50);
    }
    document
      .querySelector('[data-testid="wallet-setup-step-btn-next"]')
      .click();
    await delay(200);
  }
  //LOG info about wallet
  console.log("%c mnemonic ", "background: #222; color: #bada55");
  console.log(JSON.stringify(mnemonic).replaceAll('"', "'"));
  await delay(3000);
  console.log("%c wallet ", "background: #222; color: #bada55");
  console.log(localStorage.getItem("wallet"));
  console.log("%c keyAgentData ", "background: #222; color: #bada55");
  console.log(localStorage.getItem("keyAgentData"));
  console.log("%c lock ", "background: #222; color: #bada55");
  console.log(localStorage.getItem("lock"));
  const backgroundStorage = await chrome.storage.local.get(
    "BACKGROUND_STORAGE"
  );
  console.log(
    "%c backgroundStorage / mnemonic ",
    "background: #222; color: #bada55"
  );
  console.log(backgroundStorage["BACKGROUND_STORAGE"]["mnemonic"]);
  console.log(
    "%c backgroundStorage / keyAgentsByChain ",
    "background: #222; color: #bada55"
  );
  console.log(
    JSON.stringify(backgroundStorage["BACKGROUND_STORAGE"]["keyAgentsByChain"])
  );
})();
