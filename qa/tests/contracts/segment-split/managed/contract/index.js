import * as __compactRuntime from '@midnight-ntwrk/compact-runtime';
__compactRuntime.checkRuntimeVersion('0.16.0');

const _descriptor_0 = new __compactRuntime.CompactTypeUnsignedInteger(65535n, 2);

const _descriptor_1 = new __compactRuntime.CompactTypeBytes(32);

const _descriptor_2 = new __compactRuntime.CompactTypeUnsignedInteger(18446744073709551615n, 8);

const _descriptor_3 = __compactRuntime.CompactTypeBoolean;

class _Either_0 {
  alignment() {
    return _descriptor_3
      .alignment()
      .concat(_descriptor_1.alignment().concat(_descriptor_1.alignment()));
  }
  fromValue(value_0) {
    return {
      is_left: _descriptor_3.fromValue(value_0),
      left: _descriptor_1.fromValue(value_0),
      right: _descriptor_1.fromValue(value_0),
    };
  }
  toValue(value_0) {
    return _descriptor_3
      .toValue(value_0.is_left)
      .concat(_descriptor_1.toValue(value_0.left).concat(_descriptor_1.toValue(value_0.right)));
  }
}

const _descriptor_4 = new _Either_0();

const _descriptor_5 = new __compactRuntime.CompactTypeUnsignedInteger(
  340282366920938463463374607431768211455n,
  16,
);

class _ContractAddress_0 {
  alignment() {
    return _descriptor_1.alignment();
  }
  fromValue(value_0) {
    return {
      bytes: _descriptor_1.fromValue(value_0),
    };
  }
  toValue(value_0) {
    return _descriptor_1.toValue(value_0.bytes);
  }
}

const _descriptor_6 = new _ContractAddress_0();

const _descriptor_7 = new __compactRuntime.CompactTypeUnsignedInteger(255n, 1);

export class Contract {
  witnesses;
  constructor(...args_0) {
    if (args_0.length !== 1) {
      throw new __compactRuntime.CompactError(
        `Contract constructor: expected 1 argument, received ${args_0.length}`,
      );
    }
    const witnesses_0 = args_0[0];
    if (typeof witnesses_0 !== 'object') {
      throw new __compactRuntime.CompactError(
        'first (witnesses) argument to Contract constructor is not an object',
      );
    }
    this.witnesses = witnesses_0;
    this.circuits = {
      burnWithGuaranteed: (...args_1) => {
        if (args_1.length !== 1) {
          throw new __compactRuntime.CompactError(
            `burnWithGuaranteed: expected 1 argument (as invoked from Typescript), received ${args_1.length}`,
          );
        }
        const contextOrig_0 = args_1[0];
        if (!(
          typeof contextOrig_0 === 'object' && contextOrig_0.currentQueryContext != undefined
        )) {
          __compactRuntime.typeError(
            'burnWithGuaranteed',
            'argument 1 (as invoked from Typescript)',
            'segment-split.compact line 29 char 1',
            'CircuitContext',
            contextOrig_0,
          );
        }
        const context = { ...contextOrig_0, gasCost: __compactRuntime.emptyRunningCost() };
        const partialProofData = {
          input: { value: [], alignment: [] },
          output: undefined,
          publicTranscript: [],
          privateTranscriptOutputs: [],
        };
        const result_0 = this._burnWithGuaranteed_0(context, partialProofData);
        partialProofData.output = { value: [], alignment: [] };
        return {
          result: result_0,
          context: context,
          proofData: partialProofData,
          gasCost: context.gasCost,
        };
      },
      burnWithoutGuaranteed: (...args_1) => {
        if (args_1.length !== 1) {
          throw new __compactRuntime.CompactError(
            `burnWithoutGuaranteed: expected 1 argument (as invoked from Typescript), received ${args_1.length}`,
          );
        }
        const contextOrig_0 = args_1[0];
        if (!(
          typeof contextOrig_0 === 'object' && contextOrig_0.currentQueryContext != undefined
        )) {
          __compactRuntime.typeError(
            'burnWithoutGuaranteed',
            'argument 1 (as invoked from Typescript)',
            'segment-split.compact line 84 char 1',
            'CircuitContext',
            contextOrig_0,
          );
        }
        const context = { ...contextOrig_0, gasCost: __compactRuntime.emptyRunningCost() };
        const partialProofData = {
          input: { value: [], alignment: [] },
          output: undefined,
          publicTranscript: [],
          privateTranscriptOutputs: [],
        };
        const result_0 = this._burnWithoutGuaranteed_0(context, partialProofData);
        partialProofData.output = { value: [], alignment: [] };
        return {
          result: result_0,
          context: context,
          proofData: partialProofData,
          gasCost: context.gasCost,
        };
      },
    };
    this.impureCircuits = {
      burnWithGuaranteed: this.circuits.burnWithGuaranteed,
      burnWithoutGuaranteed: this.circuits.burnWithoutGuaranteed,
    };
    this.provableCircuits = {
      burnWithGuaranteed: this.circuits.burnWithGuaranteed,
      burnWithoutGuaranteed: this.circuits.burnWithoutGuaranteed,
    };
  }
  initialState(...args_0) {
    if (args_0.length !== 1) {
      throw new __compactRuntime.CompactError(
        `Contract state constructor: expected 1 argument (as invoked from Typescript), received ${args_0.length}`,
      );
    }
    const constructorContext_0 = args_0[0];
    if (typeof constructorContext_0 !== 'object') {
      throw new __compactRuntime.CompactError(
        `Contract state constructor: expected 'constructorContext' in argument 1 (as invoked from Typescript) to be an object`,
      );
    }
    if (!('initialZswapLocalState' in constructorContext_0)) {
      throw new __compactRuntime.CompactError(
        `Contract state constructor: expected 'initialZswapLocalState' in argument 1 (as invoked from Typescript)`,
      );
    }
    if (typeof constructorContext_0.initialZswapLocalState !== 'object') {
      throw new __compactRuntime.CompactError(
        `Contract state constructor: expected 'initialZswapLocalState' in argument 1 (as invoked from Typescript) to be an object`,
      );
    }
    const state_0 = new __compactRuntime.ContractState();
    let stateValue_0 = __compactRuntime.StateValue.newArray();
    stateValue_0 = stateValue_0.arrayPush(__compactRuntime.StateValue.newNull());
    stateValue_0 = stateValue_0.arrayPush(__compactRuntime.StateValue.newNull());
    stateValue_0 = stateValue_0.arrayPush(__compactRuntime.StateValue.newNull());
    stateValue_0 = stateValue_0.arrayPush(__compactRuntime.StateValue.newNull());
    state_0.data = new __compactRuntime.ChargedState(stateValue_0);
    state_0.setOperation('burnWithGuaranteed', new __compactRuntime.ContractOperation());
    state_0.setOperation('burnWithoutGuaranteed', new __compactRuntime.ContractOperation());
    const context = __compactRuntime.createCircuitContext(
      __compactRuntime.dummyContractAddress(),
      constructorContext_0.initialZswapLocalState.coinPublicKey,
      state_0.data,
      constructorContext_0.initialPrivateState,
    );
    const partialProofData = {
      input: { value: [], alignment: [] },
      output: undefined,
      publicTranscript: [],
      privateTranscriptOutputs: [],
    };
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_7.toValue(0n),
            alignment: _descriptor_7.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_7.toValue(1n),
            alignment: _descriptor_7.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_7.toValue(2n),
            alignment: _descriptor_7.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_7.toValue(3n),
            alignment: _descriptor_7.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newMap(new __compactRuntime.StateMap()).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
    ]);
    const tmp_0 = 1n;
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(1n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        addi: {
          immediate: parseInt(
            __compactRuntime.valueToBigInt(
              { value: _descriptor_0.toValue(tmp_0), alignment: _descriptor_0.alignment() }.value,
            ),
          ),
        },
      },
      { ins: { cached: true, n: 1 } },
    ]);
    const tmp_1 = 1n;
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(2n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        addi: {
          immediate: parseInt(
            __compactRuntime.valueToBigInt(
              { value: _descriptor_0.toValue(tmp_1), alignment: _descriptor_0.alignment() }.value,
            ),
          ),
        },
      },
      { ins: { cached: true, n: 1 } },
    ]);
    state_0.data = new __compactRuntime.ChargedState(context.currentQueryContext.state.state);
    return {
      currentContractState: state_0,
      currentPrivateState: context.currentPrivateState,
      currentZswapLocalState: context.currentZswapLocalState,
    };
  }
  _burnWithGuaranteed_0(context, partialProofData) {
    const tmp_0 = 1n;
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(0n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        addi: {
          immediate: parseInt(
            __compactRuntime.valueToBigInt(
              { value: _descriptor_0.toValue(tmp_0), alignment: _descriptor_0.alignment() }.value,
            ),
          ),
        },
      },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, ['ckpt']);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 48, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 48, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 48, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 48, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 48, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 48, 53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 48, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 48, 55, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 48, 56, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 48, 57, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 49, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 49, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 49, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 49, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 49, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 49, 53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 49, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 49, 55, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 49, 56, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 49, 57, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 50, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 50, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 50, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 50, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 50, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 50, 53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 50, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 50, 55, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 50, 56, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 50, 57, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 51, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 51, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 51, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 51, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 51, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 51, 53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 51, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 51, 55, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 51, 56, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 51, 57, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 52, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 52, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 52, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 52, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 52, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 52, 53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 52, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                103, 52, 55, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    const tmp_1 = 1n;
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(1n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        subi: {
          immediate: parseInt(
            __compactRuntime.valueToBigInt(
              { value: _descriptor_0.toValue(tmp_1), alignment: _descriptor_0.alignment() }.value,
            ),
          ),
        },
      },
      { ins: { cached: true, n: 1 } },
    ]);
    return [];
  }
  _burnWithoutGuaranteed_0(context, partialProofData) {
    __compactRuntime.queryLedgerState(context, partialProofData, ['ckpt']);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 48, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 48, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 48, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 48, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 48, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 48, 53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 48, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 48, 55, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 48, 56, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 48, 57, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 49, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 49, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 49, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 49, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 49, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 49, 53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 49, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 49, 55, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 49, 56, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 49, 57, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 50, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 50, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 50, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 50, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 50, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 50, 53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 50, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 50, 55, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 50, 56, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 50, 57, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 51, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 51, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 51, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 51, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 51, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 51, 53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 51, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 51, 55, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 51, 56, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 51, 57, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 52, 48, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 52, 49, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 52, 50, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 52, 51, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 52, 52, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 52, 53, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 52, 54, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(3n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        push: {
          storage: false,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_1.toValue(
              new Uint8Array([
                110, 52, 55, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
                0, 0, 0, 0, 0,
              ]),
            ),
            alignment: _descriptor_1.alignment(),
          }).encode(),
        },
      },
      {
        push: {
          storage: true,
          value: __compactRuntime.StateValue.newCell({
            value: _descriptor_2.toValue(0n),
            alignment: _descriptor_2.alignment(),
          }).encode(),
        },
      },
      { ins: { cached: false, n: 1 } },
      { ins: { cached: true, n: 1 } },
    ]);
    const tmp_0 = 1n;
    __compactRuntime.queryLedgerState(context, partialProofData, [
      {
        idx: {
          cached: false,
          pushPath: true,
          path: [
            {
              tag: 'value',
              value: { value: _descriptor_7.toValue(2n), alignment: _descriptor_7.alignment() },
            },
          ],
        },
      },
      {
        subi: {
          immediate: parseInt(
            __compactRuntime.valueToBigInt(
              { value: _descriptor_0.toValue(tmp_0), alignment: _descriptor_0.alignment() }.value,
            ),
          ),
        },
      },
      { ins: { cached: true, n: 1 } },
    ]);
    return [];
  }
}
export function ledger(stateOrChargedState) {
  const state =
    stateOrChargedState instanceof __compactRuntime.StateValue
      ? stateOrChargedState
      : stateOrChargedState.state;
  const chargedState =
    stateOrChargedState instanceof __compactRuntime.StateValue
      ? new __compactRuntime.ChargedState(stateOrChargedState)
      : stateOrChargedState;
  const context = {
    currentQueryContext: new __compactRuntime.QueryContext(
      chargedState,
      __compactRuntime.dummyContractAddress(),
    ),
    costModel: __compactRuntime.CostModel.initialCostModel(),
  };
  const partialProofData = {
    input: { value: [], alignment: [] },
    output: undefined,
    publicTranscript: [],
    privateTranscriptOutputs: [],
  };
  return {
    get guaranteedMarker() {
      return _descriptor_2.fromValue(
        __compactRuntime.queryLedgerState(context, partialProofData, [
          { dup: { n: 0 } },
          {
            idx: {
              cached: false,
              pushPath: false,
              path: [
                {
                  tag: 'value',
                  value: { value: _descriptor_7.toValue(0n), alignment: _descriptor_7.alignment() },
                },
              ],
            },
          },
          { popeq: { cached: true, result: undefined } },
        ]).value,
      );
    },
    get fuseWithGuaranteed() {
      return _descriptor_2.fromValue(
        __compactRuntime.queryLedgerState(context, partialProofData, [
          { dup: { n: 0 } },
          {
            idx: {
              cached: false,
              pushPath: false,
              path: [
                {
                  tag: 'value',
                  value: { value: _descriptor_7.toValue(1n), alignment: _descriptor_7.alignment() },
                },
              ],
            },
          },
          { popeq: { cached: true, result: undefined } },
        ]).value,
      );
    },
    get fuseWithoutGuaranteed() {
      return _descriptor_2.fromValue(
        __compactRuntime.queryLedgerState(context, partialProofData, [
          { dup: { n: 0 } },
          {
            idx: {
              cached: false,
              pushPath: false,
              path: [
                {
                  tag: 'value',
                  value: { value: _descriptor_7.toValue(2n), alignment: _descriptor_7.alignment() },
                },
              ],
            },
          },
          { popeq: { cached: true, result: undefined } },
        ]).value,
      );
    },
    ballast: {
      isEmpty(...args_0) {
        if (args_0.length !== 0) {
          throw new __compactRuntime.CompactError(
            `isEmpty: expected 0 arguments, received ${args_0.length}`,
          );
        }
        return _descriptor_3.fromValue(
          __compactRuntime.queryLedgerState(context, partialProofData, [
            { dup: { n: 0 } },
            {
              idx: {
                cached: false,
                pushPath: false,
                path: [
                  {
                    tag: 'value',
                    value: {
                      value: _descriptor_7.toValue(3n),
                      alignment: _descriptor_7.alignment(),
                    },
                  },
                ],
              },
            },
            'size',
            {
              push: {
                storage: false,
                value: __compactRuntime.StateValue.newCell({
                  value: _descriptor_2.toValue(0n),
                  alignment: _descriptor_2.alignment(),
                }).encode(),
              },
            },
            'eq',
            { popeq: { cached: true, result: undefined } },
          ]).value,
        );
      },
      size(...args_0) {
        if (args_0.length !== 0) {
          throw new __compactRuntime.CompactError(
            `size: expected 0 arguments, received ${args_0.length}`,
          );
        }
        return _descriptor_2.fromValue(
          __compactRuntime.queryLedgerState(context, partialProofData, [
            { dup: { n: 0 } },
            {
              idx: {
                cached: false,
                pushPath: false,
                path: [
                  {
                    tag: 'value',
                    value: {
                      value: _descriptor_7.toValue(3n),
                      alignment: _descriptor_7.alignment(),
                    },
                  },
                ],
              },
            },
            'size',
            { popeq: { cached: true, result: undefined } },
          ]).value,
        );
      },
      member(...args_0) {
        if (args_0.length !== 1) {
          throw new __compactRuntime.CompactError(
            `member: expected 1 argument, received ${args_0.length}`,
          );
        }
        const key_0 = args_0[0];
        if (!(
          key_0.buffer instanceof ArrayBuffer &&
          key_0.BYTES_PER_ELEMENT === 1 &&
          key_0.length === 32
        )) {
          __compactRuntime.typeError(
            'member',
            'argument 1',
            'segment-split.compact line 21 char 1',
            'Bytes<32>',
            key_0,
          );
        }
        return _descriptor_3.fromValue(
          __compactRuntime.queryLedgerState(context, partialProofData, [
            { dup: { n: 0 } },
            {
              idx: {
                cached: false,
                pushPath: false,
                path: [
                  {
                    tag: 'value',
                    value: {
                      value: _descriptor_7.toValue(3n),
                      alignment: _descriptor_7.alignment(),
                    },
                  },
                ],
              },
            },
            {
              push: {
                storage: false,
                value: __compactRuntime.StateValue.newCell({
                  value: _descriptor_1.toValue(key_0),
                  alignment: _descriptor_1.alignment(),
                }).encode(),
              },
            },
            'member',
            { popeq: { cached: true, result: undefined } },
          ]).value,
        );
      },
      lookup(...args_0) {
        if (args_0.length !== 1) {
          throw new __compactRuntime.CompactError(
            `lookup: expected 1 argument, received ${args_0.length}`,
          );
        }
        const key_0 = args_0[0];
        if (!(
          key_0.buffer instanceof ArrayBuffer &&
          key_0.BYTES_PER_ELEMENT === 1 &&
          key_0.length === 32
        )) {
          __compactRuntime.typeError(
            'lookup',
            'argument 1',
            'segment-split.compact line 21 char 1',
            'Bytes<32>',
            key_0,
          );
        }
        if (
          state
            .asArray()[3]
            .asMap()
            .get({ value: _descriptor_1.toValue(key_0), alignment: _descriptor_1.alignment() }) ===
          undefined
        ) {
          throw new __compactRuntime.CompactError(`Map value undefined for ${key_0}`);
        }
        return {
          read(...args_1) {
            if (args_1.length !== 0) {
              throw new __compactRuntime.CompactError(
                `read: expected 0 arguments, received ${args_1.length}`,
              );
            }
            return _descriptor_2.fromValue(
              __compactRuntime.queryLedgerState(context, partialProofData, [
                { dup: { n: 0 } },
                {
                  idx: {
                    cached: false,
                    pushPath: false,
                    path: [
                      {
                        tag: 'value',
                        value: {
                          value: _descriptor_7.toValue(3n),
                          alignment: _descriptor_7.alignment(),
                        },
                      },
                      {
                        tag: 'value',
                        value: {
                          value: _descriptor_1.toValue(key_0),
                          alignment: _descriptor_1.alignment(),
                        },
                      },
                    ],
                  },
                },
                { popeq: { cached: true, result: undefined } },
              ]).value,
            );
          },
        };
      },
    },
  };
}
const _emptyContext = {
  currentQueryContext: new __compactRuntime.QueryContext(
    new __compactRuntime.ContractState().data,
    __compactRuntime.dummyContractAddress(),
  ),
};
const _dummyContract = new Contract({});
export const pureCircuits = {};
export const contractReferenceLocations = { tag: 'publicLedgerArray', indices: {} };
//# sourceMappingURL=index.js.map
