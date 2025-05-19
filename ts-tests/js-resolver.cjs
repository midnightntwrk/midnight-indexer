//  enables mixed JS and TS codebase
const jsResolver = (path, options) => {
  const jsExtRegex = /\.js$/i;
  const resolver = options.defaultResolver;
  // eslint-disable-next-line @typescript-eslint/no-unsafe-argument
  if (jsExtRegex.test(path) && !options.basedir.includes('node_modules') && !path.includes('node_modules')) {
    const newPath = path.replace(jsExtRegex, '.ts');
    try {
      return resolver(newPath, options);
    } catch {
      // use default resolver
    }
  }

  return resolver(path, options);
};

module.exports = jsResolver;
