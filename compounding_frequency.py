
import scipy.optimize
import numpy
import plotnine
import pandas

def generate_root_find_function(r, t):
    def root_find_function(n):
        x = r*t
        lhs = 1 + x + x*x/2 + x*x*x/6
        #lhs = numpy.exp(x)
        rhs = numpy.float_power(1+r/n, n*t)
        return lhs - rhs

    return root_find_function

def generate_root_find_function_first_deriv(r, t):
    def root_find_function_first_deriv(n):
        return -1*( t * numpy.power((n + r) / n, n * t) * ( ( n + r ) * numpy.log((n + r)/n) - r) ) / (n + r)

    return root_find_function_first_deriv


def newton_root(r, t):
    return scipy.optimize.newton(generate_root_find_function(r, t), 1, maxiter=256, tol=0.1)

def newton():
    rates = [0.03, 0.05, 0.08]
    data = dict()
    times = numpy.linspace(1/2, 1, 20)
    xlab = 'Time in Years'
    ylab = 'Number of times compounded in one day'

    for r in rates:
        roots = [ newton_root(r, t) for t in times ]
        data[str(r)] = numpy.array(roots) / 365.0

    data[xlab] = times

    data = pandas.DataFrame(data)

    plotdf = data.melt(id_vars=xlab, var_name='R', value_name=ylab)

    print(
        plotnine.ggplot(plotdf, plotnine.aes(x=xlab, y=ylab, colour='R'))
        + plotnine.geom_line()
        + plotnine.ggtitle('Time in years vs number of times compounded in one day at various rates')
    )
    print(data.tail(5))

def numerical_instability():
    r = 0.2
    t = 1.0
    num_points = 1000

    root_find_function = generate_root_find_function(r, t)
    xax = numpy.linspace(1e6, 1e8, num=num_points)
    estimated_value = root_find_function(xax)
    true_value = numpy.ones(num_points) * numpy.exp(r*t)

    plotdf = pandas.DataFrame({'CompoundingFreq': xax, 'EstVal': estimated_value, 'TrueVal': true_value})
    plotdf = plotdf.melt(id_vars=['CompoundingFreq'], var_name='Type', value_name='Estimate')
    print(plotnine.ggplot(plotdf, plotnine.aes(x = 'CompoundingFreq', y = 'Estimate', colour='Type'))
          + plotnine.geom_line()
          )



def repeated_multiplication():
    r = 0.2
    t = 1.0 # DO NOT CHANGE (ASSUMED IN LOOP BELOW)
    num_points = 1000
    nstart = int(1e4)
    nstop = int(1e8)
    nstep = int((nstop - nstart) / num_points)
    all_n = numpy.array(list(range(nstart, nstop, nstep)))
    num_points = len(all_n)
    estimated_value = []

    for n in all_n:
        estimated_value.append(numpy.product(numpy.ones(n)*(1 + r / n)))

    xax = all_n
    true_value = numpy.ones(num_points) * numpy.exp(r*t)

    plotdf = pandas.DataFrame({'CompoundingFreq': xax, 'EstVal': estimated_value, 'TrueVal': true_value})
    plotdf = plotdf.melt(id_vars=['CompoundingFreq'], var_name='Type', value_name='Estimate')
    print(plotnine.ggplot(plotdf, plotnine.aes(x = 'CompoundingFreq', y = 'Estimate', colour='Type'))
          + plotnine.geom_line()
          )



if __name__ == '__main__':
    newton()