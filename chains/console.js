const repl = require('repl');
const path = require('path');
const { Readable } = require('stream');
const { createReadStream } = require('fs');
const getopts = require('getopts');

function matchesLine(completion, line) {
  if (completion.initial && line.startsWith(completion.initial)) {
    return true;
  }

  return false;
}

function targetMatches(completion, line) {
  let words = line.split(/\s+/);
  let position = words.length - 1; // e.g. "deploy" = 0 "deploy " = 1 "deploy abc" = 1
  let lastWord = words.length === 0 ? "" : words[words.length - 1];
  let targets = completion.targets.filter(({pos}) => pos === position);

  let matching = targets.reduce((acc, {choices}) => {
    return [
      ...acc,
      ...choices.filter((choice) => choice.startsWith(lastWord))
    ];
  }, []);

  if (lastWord.length > 0) {
    return [matching, lastWord];
  } else {
    return [matching, line];
  }
}

function getCompleter(defaultCompleter, completions) {
  return function(line, callback) {
    const lineMatches = completions.filter((completion) => matchesLine(completion, line));
    let [choices, text] = lineMatches.reduce(([accMatch, accText], completion) => {
      let [matches, text] = targetMatches(completion, line);

      if (matches && text.length < accText.length) {
        return [matches, text];
      } else if (matches && text.length === accText.length) {
        return [ [ ...accMatch, ...matches ], accText];
      } else {
        return [accMatch, accText];
      }
    }, [[], line]);

    if (choices.length > 0) {
      callback(null, [choices, text]);
    } else {
      defaultCompleter(line, callback);
    }
  }
}

function getCompletions(defaultCompleter, contracts) {
  let contractNames = Object.keys(contracts)
  let contractAddresses = Object.values(contracts).filter((x) => !!x);

  const completions = [
    {
      initial: '.deploy',
      targets: [
        {
          pos: 1,
          choices: contractNames
        }
      ]
    },
    {
      initial: '.match',
      targets: [
        {
          pos: 1,
          choices: contractAddresses
        },
        {
          pos: 2,
          choices: contractNames
        }
      ]
    }
  ];

  return getCompleter(defaultCompleter, completions);
}

function lowerCase(str) {
  if (str === "") {
    return "";
  } else {
    return str[0].toLowerCase() + str.slice(1,);
  }
}

async function wrapError(p, that) {
  try {
    return await p;
  } catch (err) {
    console.error(`Error: ${err}`);
  } finally {
    that.displayPrompt();
  }
}

async function getContracts(saddle) {
  // let contracts = await saddle.listContracts();
  // let contractInsts = await Object.entries(contracts).reduce(async (acc, [contract, address]) => {
  //   if (address) {
  //     return {
  //       ... await acc,
  //       [contract]: await saddle.getContractAt(contract, address)
  //     };
  //   } else {
  //     return await acc;
  //   }
  // }, <Promise<{[name: string]: Contract}>>{});

  // return {
  //   contracts,
  //   contractInsts
  // }
  return {
    contracts: [],
    contractInsts: []
  };
}

function defineCommands(r, saddle, network, contracts) {
  r.defineCommand('deploy', {
    help: 'Deploy a given contract',
    action(...args) {
      this.clearBufferedCommand();
      let that = this;

      getCli().parse(`deploy -n ${network} ${args.join(" ")}`, function (err, argv, output) {
        if (err) {
          console.error(`Error: ${err}`);
        } else {
          console.log(output);
          wrapError(argv.deployResult, that).then((res) => {
            if (res) {
              getContracts(saddle).then(({contracts, contractInsts}) => {
                r.completer = getCompletions(r.originalCompleter, contracts);
                defineContracts(r, saddle, contractInsts);
                defineCommands(r, saddle, network, contracts);

                that.displayPrompt();
              });
            }
          });
        }
      });
    }
  });

  r.defineCommand('verify', {
    help: 'Verify a given contract on Etherscan',
    action(...args) {
      this.clearBufferedCommand();
      let that = this;

      getCli().parse(`verify -n ${network} ${args.join(" ")}`, function (err, argv, output) {
        if (err) {
          console.error(`Error: ${err}`);
        } else {
          console.log(output);
          wrapError(argv.verifyResult, that);
        }
      });
    }
  });

  r.defineCommand('match', {
    help: 'Matches a given contract to an Ethereum deploy contract',
    action(...args) {
      this.clearBufferedCommand();
      let that = this;

      getCli().parse(`match -n ${network} ${args.join(" ")}`, function (err, argv, output) {
        if (err) {
          console.error(`Error: ${err}`);
        } else {
          console.log(output);
          wrapError(argv.matchResult, that);
        }
      });
    }
  });

  r.defineCommand('compile', {
    help: 'Re-compile contracts',
    action(name) {
      this.clearBufferedCommand();
      let that = this;

      getCli().parse(`compile ${name}`, function (err, argv, output) {
        if (err) {
          console.error(`Error: ${err}`);
        } else {
          console.log(output);
          wrapError(argv.compileResult, that).then((res) => {
            if (res) {
              getContracts(saddle).then(({contracts, contractInsts}) => {
                r.completer = getCompletions(r.originalCompleter, contracts);
                defineContracts(r, saddle, contractInsts);
                defineCommands(r, saddle, network, contracts);

                that.displayPrompt();
              });
            }
          });
        }
      });
    }
  });

  r.defineCommand('contracts', {
    help: 'Lists known contracts',
    action(name) {
      this.clearBufferedCommand();
      let that = this;

      getCli().parse(`contracts ${name}`, function (err, argv, output) {
        if (err) {
          console.error(`Error: ${err}`);
        } else {
          console.log(output);
          wrapError(argv.contractsResult, that);
        }
      });
    }
  });

  r.defineCommand('network', {
    help: 'Show given network',
    action(name) {
      this.clearBufferedCommand();
      console.log(`Network: ${network}`);
      this.displayPrompt();
    }
  });

  r.defineCommand('provider', {
    help: 'Show given provider',
    action(name) {
      this.clearBufferedCommand();
      console.log(`Provider: ${describeProvider(saddle.web3.currentProvider)}`);
      this.displayPrompt();
    }
  });

  r.defineCommand('from', {
    help: 'Show default from address',
    action(name) {
      this.clearBufferedCommand();
      console.log(`From: ${saddle.network_config.default_account}`);
      this.displayPrompt();
    }
  });

  r.defineCommand('deployed', {
    help: 'Show given deployed contracts',
    action(name) {
      this.clearBufferedCommand();
      Object.entries(contracts).forEach(([contract, deployed]) => {
        console.log(`${contract}: ${deployed || ""}`);
      });
      this.displayPrompt();
    }
  });
}

function defineContracts(r, saddle, contractInsts) {
  Object.entries(contractInsts).forEach(([contract, contractInst]) => {
    Object.defineProperty(r.context, lowerCase(contract), {
      configurable: true,
      enumerable: true,
      value: contractInst
    });
  });
}

function connect(chain) {
  const wsProvider = new WsProvider(`ws://localhost:${this.wsPort}`);
  const api = await ApiPromise.create({
    provider: wsProvider,
    types: await loadTypes(this.ctx)
  });

  return {
    wsProvider,
    api
  };
}

async function startConsole(input, chain, trace) {
  // let saddle = await getSaddle(network);
  let {contracts, contractInsts} = await getContracts({});

  console.info(`Gateway console on chain ${chain}`);
  console.info(`Deployed ${network} contracts`);

  Object.entries(contracts).forEach(([contract, deployed]) => {
    if (deployed) {
      console.log(`\t${lowerCase(contract)}: ${deployed}`);
    }
  });

  let r = repl.start({
    prompt: '> ',
    input: input,
    output: input ? process.stdout : undefined,
    terminal: input ? false : undefined
  });
  if (typeof(r.setupHistory) === 'function') {
    r.setupHistory(path.join(process.cwd(), '.gateway_history'), (err, repl) => null);
  }
  r.originalCompleter = r.completer;
  r.completer = getCompletions(r.completer, contracts);

  // defineCommands(r, saddle, network, contracts);

  let connection = await connect(chain);

  Object.entires(connection).forEach(([key, value]) => {
    Object.defineProperty(r.context, key, {
      configurable: false,
      enumerable: typeof(value) !== 'function',
      value
    });
  });

  // defineContracts(r, saddle, contractInsts);

  process.on('uncaughtException', () => console.log('Error'));
}

let input;
const options = getopts(process.argv.slice(2), {
  alias: {
    script: "s",
    eval: "e",
    chain: "c"
  },
});

if (!options.chain) {
  throw new Error(`Must choose chain with -c`);
}

if (options.script) {
  input = createReadStream(scriptArg);
} else if (options.eval) {
  let evalArg = options.eval;
  let codes = Array.isArray(evalArg) ? evalArg.map((e) => e + ';\n') : [ evalArg ];
  input = Readable.from(codes);
}

startConsole(input, options.chain, process.env['TRACE']);
