// First
//let mnemonic = [
//   "attend",
//   "unknown",
//   "radar",
//   "furnace",
//   "young",
//   "half",
//   "conduct",
//   "hammer",
//   "build",
//   "stock",
//   "used",
//   "ocean",
//   "bleak",
//   "shuffle",
//   "mandate",
//   "where",
//   "field",
//   "settle",
//   "tool",
//   "despair",
//   "buddy",
//   "true",
//   "lottery",
//   "toast",
// ];
// New let mnemonic = ['work', 'choose', 'illness', 'flock', 'rug', 'wood', 'cash', 'magic', 'prosper', 'penalty', 'verify', 'entry', 'balcony', 'spy', 'ozone', 'list', 'betray', 'hollow', 'pond', 'key', 'number', 'marine', 'mosquito', 'trouble'];
// Kiryl let mnemonic = ['supply', 'album', 'trouble', 'paper', 'limit', 'elbow', 'zone', 'slim', 'dose', 'match', 'prepare', 'lock', 'slot', 'bleak', 'thrive', 'must', 'tower', 'border', 'width', 'head', 'buzz', 'alpha', 'soon', 'cushion'];
// Monika let mnemonic = ['increase', 'crane', 'seminar', 'produce', 'hurdle', 'vessel', 'chimney', 'mad', 'farm', 'safe', 'garment', 'wealth', 'ripple', 'what', 'orchard', 'mountain', 'forward', 'senior', 'lecture', 'jar', 'invest', 'fortune', 'amateur', 'ride'];
//Pre-funded 
// let mnemonic = ['abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'abandon', 'absurd', 'fetch'];

let mnemonic = [
  'abandon', 'abandon', 'abandon',
  'abandon', 'abandon', 'abandon',
  'abandon', 'abandon', 'abandon',
  'abandon', 'abandon', 'abandon',
  'abandon', 'abandon', 'abandon',
  'abandon', 'abandon', 'abandon',
  'abandon', 'abandon', 'abandon',
  'abandon', 'abandon', 'diesel'
];

// Yevhen let mnemonic = ['once', 'write', 'job', 'pepper', 'one', 'choose', 'lunch', 'devote', 'escape', 'wild', 'release', 'sick', 'until', 'swamp', 'knock', 'left', 'ensure', 'utility', 'act', 'common', 'plate', 'together', 'comic', 'hammer'];
// newest let mnemonic = ['once', 'write', 'job', 'pepper', 'one', 'choose', 'lunch', 'devote', 'escape', 'wild', 'release', 'sick', 'until', 'swamp', 'knock', 'left', 'ensure', 'utility', 'act', 'common', 'plate', 'together', 'comic', 'comic'];

const walletName = "test";

const walletPassword = "123456789test!!";
const nodeAddress = "http://localhost:9944";
const proverAddress = "http://localhost:6300";
const indexerAddress = "http://localhost:8088/api/v1/graphql";
const offset = [0, 8, 16];
const delay = (ms) => new Promise((res) => setTimeout(res, ms));

(async () => {
  document.querySelector('[data-testid="restore-wallet-button"]').click();
  await delay(300);
  document
    .querySelector('[data-testid="delete-address-modal-confirm"]')
    .click();
  await delay(300);
  document
    .querySelector('[data-testid="wallet-setup-legal-terms-checkbox"]')
    .click();
  await delay(300);
  document.querySelector('[data-testid="wallet-setup-step-btn-next"]').click();
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
  // Update node, pubsub, prove-server addresses
  document
    .querySelector('[data-testid="network-undeployed-radio-button"]')
    .click();
  await delay(300);
  document
    .querySelector('[data-testid="midnight-wallet-address-input"]')
    .focus();
  document.execCommand("selectall", "false", null);
  document.execCommand("insertText", "false", nodeAddress);
  await delay(300);
  document
    .querySelector('[data-testid="pubsub-indexer-address-input"]')
    .focus();
  document.execCommand("selectall", "false", null);
  document.execCommand("insertText", "false", indexerAddress);
  await delay(300);
  document
    .querySelector('[data-testid="proving-server-address-input"]')
    .focus();
  document.execCommand("selectall", "false", null);
  document.execCommand("insertText", "false", proverAddress);
  await delay(300);
  document.querySelector('[data-testid="wallet-setup-step-btn-next"]').click();
  await delay(300);
  document.querySelector('[data-testid="wallet-setup-step-btn-next"]').click();
  await delay(300);

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
    await delay(100);
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
