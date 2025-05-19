// testnet/qanet const address = "16cc95f0c76d32efeffa397a3cf59dfc633909091e2ba163d3cbcb3804de83d1|0300cdd255354ecb3048d871fe1d43ebc5d2f702ec3d0b14b30634416705dec582651cf1bfe61f1d2347b4633d551885faaebb3ffa83e9b04392";
// local
// const address =
// "105418f97290abbecb22dc91ac3a756e9035a218bde85feacaf196676c60c51e|03000d156a94621e636769b4d99ee989d9ee80bc0fdba743e97b14ed2fabaee18fd17ec487844d2a3137b3b9d4ca53a10181863998a115450e08";

const address =
  "79a3725c6767ac8514831ea07ec252b0ef5f1495983dc8b0b0f64c9c794f5daa|0300239f1babe82996d6a46a290976d361b95b9372918db9325d06fce0a9e31c110993b02715e9cee975f7201fc8c01434edeee30ac222ed1c0a";

const walletPassword = "123456789test!!";
const delay = (ms) => new Promise((res) => setTimeout(res, ms));
const loop = 1;
const amount = "1";

(async () => {
  for (let i = 0; i < loop; i++) {
    document.querySelector('[data-testid="send-button"]').click();
    await delay(500);
    document.querySelector('[data-testid="search-input"]').focus();
    document.execCommand("insertText", "true", address);
    await delay(500);
    document.querySelector('[data-testid="token-amount-input"]').focus();
    document.execCommand("insertText", "false", amount);
    await delay(300);
    document.querySelector('[data-testid="send-next-btn"]').click();
    await delay(300);
    document.querySelector('[data-testid="send-next-btn"]').click();
    await delay(300);
    document.querySelector('[data-testid="password-input"]').focus();
    document.execCommand("insertText", "false", walletPassword);
    await delay(300);
    document
      .querySelector(
        "div.ConfirmTxModal-module_buttons__4iGlN > button:nth-child(1)"
      )
      .click();
    await delay(1000);

    while (
      document.querySelector(
        "div > div.SendTransaction-module_footer__3QNzs > button:nth-child(2)"
      )
    ) {
      await delay(1000);
    }

    await delay(1000);
  }
})();
